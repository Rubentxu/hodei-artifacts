//! La superficie OCI Distribution de Hodei, en el wire.
//!
//! `hodei-hub::oci` lee manifiestos como **tipos**; esto es el
//! **protocolo**: los endpoints `/v2` de la distribucion, servidos desde
//! el catalogo y el almacen de bytes que ya existen. No hay un
//! almacenamiento de imagen propio: un manifiesto es un asset del
//! repositorio, un blob es un asset que el repositorio referencia, y una
//! etiqueta es la referencia mutable que el catalogo resuelve.
//!
//! El corte de **pull** son las lecturas: `GET /v2/`, blobs y manifiestos
//! por digest y por etiqueta, con la negociacion de tipos. El corte de
//! **push** --las sesiones de subida y el manifiesto-- esta en
//! [`crate::oci::push`], y lo que los dos comparten --el dialecto del
//! error, el nombre, el digest, el reparto del path-- esta en
//! [`crate::oci`].
//!
//! La identidad del contenido es el digest, y la etiqueta nunca se
//! convierte en contenido: se resuelve y se olvida, y la respuesta dice
//! el digest resuelto, para que un cliente que pidio por etiqueta sepa
//! que lo que hay detras es inmutable.
//!
//! Los errores hablan **OCI**: un envelope con `code` y `message`, y los
//! codigos son los del dialecto (BLOB_UNKNOWN, MANIFEST_UNKNOWN,
//! NAME_UNKNOWN, NAME_INVALID, DIGEST_INVALID). Un repositorio de otra
//! cuenta es `NAME_UNKNOWN`, que no dice si existe en otro sitio.

use axum::body::Body;
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::app_state::AppState;
use crate::oci::failure::OciFailure;
use crate::oci::target::{Target, the_digest_of_reference, the_target};
use crate::oci::{the_catalog, the_content_store, the_context, the_decision, the_repository};

/// Los media types de manifiesto que este registro sirve.
///
/// Son los que el subconjunto de OCI declara en el codigo: el manifiesto
/// de imagen de OCI y el de Docker, el indice multi-plataforma y la lista
/// de Docker, y el manifiesto de artefacto con su `subject`.
const SUPPORTED_MANIFESTS: &[&str] = hodei_hub::oci::SUPPORTED_MANIFESTS;

/// La cabecera que anuncia la version de la API de distribucion.
const API_VERSION_HEADER: &str = "Docker-Distribution-API-Version";

/// El ping del protocolo: `GET /v2/`.
///
/// Autenticado como todo el resto de la superficie; no mira repositorios.
pub async fn version_check(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, OciFailure> {
    let _ = the_context(&state, &headers).await?;
    let mut response = (StatusCode::OK, "{}".to_string()).into_response();
    response
        .headers_mut()
        .insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(response)
}

/// El punto de entrada de la superficie.
///
/// El metodo y el path dicen que operacion es. El `GET` de una sesion de
/// subida es el **estado** de la subida, no la lectura de un blob, y por
/// eso se decide antes de leer.
pub async fn dispatch(
    State(state): State<AppState>,
    uri: OriginalUri,
    method: Method,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
) -> Result<Response, OciFailure> {
    match method {
        Method::GET | Method::HEAD => {
            if let Ok(Target::UploadSession { .. }) = the_target(uri.path()) {
                return crate::oci::push::upload_status(state, uri, headers).await;
            }
            content(state, uri, headers, method).await
        }
        _ => crate::oci::push::mutate(state, uri, method, headers, request).await,
    }
}

/// El contenido de la distribucion: blobs y manifests.
///
/// El GET y el HEAD comparten handler: el HEAD dice las mismas cabeceras
/// y no trae cuerpo, que es lo que el spec pide y lo que un cliente de
/// pull necesita para saber el tamano sin descargarlo.
pub async fn content(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    method: Method,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let head = method == Method::HEAD;

    // El nombre, la clase de recurso y la referencia salen del path en
    // crudo: `/<name>/blobs/<digest>` o `/<name>/manifests/<reference>`,
    // donde `<name>` puede tener niveles (`team/demo`).
    let target = the_target(uri.path())?;
    match target {
        Target::Blob { name, reference } => blob(&state, &context, &name, &reference, head).await,
        Target::Manifest { name, reference } => {
            manifest(&state, &context, &name, &reference, &headers, head).await
        }
        // La lista de etiquetas y los referrers no son contenido: son
        // preguntas por el repositorio, y las responde la superficie de
        // las consultas.
        Target::Tags { name } => {
            crate::oci::query::the_tags(&state, &context, &name, uri.query().unwrap_or("")).await
        }
        Target::Referrers { name, digest } => {
            crate::oci::query::the_referrers(
                &state,
                &context,
                &name,
                &digest,
                uri.query().unwrap_or(""),
            )
            .await
        }
        // El inicio de una subida y su sesion no son contenido: son el
        // estado de una subida, y se dicen en la superficie de push.
        Target::UploadStart { .. } | Target::UploadSession { .. } => Err(OciFailure::unsupported(
            "an upload is not content of the repository",
        )),
    }
}

/// Un blob del repositorio, servido por su digest.
///
/// La referencia del catálogo es la que autoriza el servir: los bytes que
/// estan en el CAS pero que el repositorio no referencia no son contenido
/// del repositorio, y se responden como BLOB_UNKNOWN.
async fn blob(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    reference: &str,
    head: bool,
) -> Result<Response, OciFailure> {
    let digest = the_digest_of_reference(reference)?;

    let repository = the_repository(state, context, name).await?;
    the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;

    // La fila del asset es la referencia: sin ella, el blob no es
    // contenido de este repositorio, esten sus bytes donde esten.
    let (reader, _) = the_catalog(state)?;
    let asset = reader
        .asset(context.account().as_str(), name, &format!("blobs/{digest}"))
        .await
        .map_err(|error| match error {
            hodei_catalog::CatalogError::Missing { .. } => {
                OciFailure::blob_unknown(format!("the repository does not reference {digest}"))
            }
            _other => OciFailure::unavailable("the catalog is not available"),
        })?;

    // Los bytes que se sirven son los que la **fila** referencia, no los
    // que dice la URL. El digest de la URL dice **que** se pide; la fila
    // dice **que** hay. Si se sirieran los del digest de la URL, una
    // fila que solo nombra una ruta serviria el contenido de otro blob,
    // que puede ser de otra cuenta: el CAS es un arbol comun.
    //
    // Por eso la respuesta lleva el digest de lo que se sirve, y por eso
    // un digest pedido que no es el referenciado es un BLOB_UNKNOWN, no
    // bytes.
    if asset.digest.value != digest {
        return Err(OciFailure::blob_unknown(format!(
            "the repository does not reference {digest}"
        )));
    }
    crate::trust::admit(
        state,
        context.account().as_str(),
        &repository,
        &asset.digest,
    )
    .await
    .map_err(|admission| {
        the_admission_failure(
            admission,
            |digest| {
                OciFailure::blob_unknown(format!("the repository does not reference {digest}"))
            },
            &digest,
        )
    })?;
    let store = the_content_store(state)?;
    let bytes = store
        .get(&asset.digest)
        .await
        .map_err(|_| OciFailure::blob_unknown(format!("the blob {digest} is not in the store")))?;
    debug_assert_eq!(
        bytes.len() as u64,
        asset.size_bytes,
        "the catalog and the store disagree about the size of the blob"
    );

    Ok(oci_bytes_response(
        bytes,
        "application/octet-stream",
        &digest,
        head,
    ))
}

/// Un manifest del repositorio, por digest o por tag.
///
/// La etiqueta es la referencia mutable que el catálogo resuelve a las
/// coordenadas; la identidad del contenido sigue siendo el digest del
/// manifiesto, y la respuesta lo dice en `Docker-Content-Digest`.
async fn manifest(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    reference: &str,
    headers: &HeaderMap,
    head: bool,
) -> Result<Response, OciFailure> {
    let repository = the_repository(state, context, name).await?;
    the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;

    let (reader, _) = the_catalog(state)?;

    // El digest del manifiesto: el que el cliente nombra, o el que la
    // etiqueta resuelve. La etiqueta nunca se convierte en contenido: se
    // resuelve y se olvida, y la respuesta dice el digest resuelto.
    let digest = if reference.contains(':') {
        the_digest_of_reference(reference)?
    } else {
        let tag = reader
            .tag(context.account().as_str(), name, reference)
            .await
            .map_err(|error| match error {
                hodei_catalog::CatalogError::Missing { .. } => the_unknown_manifest(reference),
                _other => OciFailure::unavailable("the catalog is not available"),
            })?
            .coordinates
            .package_version;
        the_digest_of_reference(&format!("sha256:{tag}"))?
    };

    let asset = reader
        .asset(
            context.account().as_str(),
            name,
            &format!("manifests/{digest}"),
        )
        .await
        .map_err(|error| match error {
            hodei_catalog::CatalogError::Missing { .. } => the_unknown_manifest(reference),
            _other => OciFailure::unavailable("the catalog is not available"),
        })?;

    // Los bytes que se sirven son los que la fila referencia, no los que
    // dice la URL: como en los blobs, el digest pedido dice **que** se
    // pide y la fila dice **que** hay.
    if asset.digest.value != digest {
        return Err(the_unknown_manifest(reference));
    }

    // La etiqueta ya se resolvio a su digest destino: lo que se admite es
    // el digest, no el nombre. Una etiqueta que se movio a otro digest no
    // hereda la concesion del anterior.
    crate::trust::admit(
        state,
        context.account().as_str(),
        &repository,
        &asset.digest,
    )
    .await
    .map_err(|admission| {
        the_admission_failure(admission, |_| the_unknown_manifest(reference), &digest)
    })?;

    let store = the_content_store(state)?;
    let bytes = store.get(&asset.digest).await.map_err(|_| {
        OciFailure::manifest_unknown(format!("the manifest {digest} is not in the store"))
    })?;

    // El tipo de medio lo dice el propio manifiesto, no el cliente: el
    // manifiesto es la verdad de que es, y el cliente lo pide con el
    // `Accept`, no lo decide.
    //
    // Se lee del JSON y no de `hodei_hub::oci::OciManifest::read`, que
    // solo admite el tipo OCI: un manifiesto Docker schema2 es un
    // manifiesto valido, y el registro lo sirve con el tipo que el
    // manifiesto declara.
    let media_type = the_declared_media_type(&bytes)?;

    // La negociacion, con el tipo que este manifiesto declara: si el
    // cliente no lo acepta, el error es de manifiesto desconocido y no
    // un 200 con un tipo que no pidio.
    the_negotiation(headers, &media_type)?;
    debug_assert_eq!(
        bytes.len() as u64,
        asset.size_bytes,
        "the catalog and the store disagree about the size of the manifest"
    );

    Ok(oci_bytes_response(bytes, &media_type, &digest, head))
}

/// El 404 de un manifiesto, con la referencia que el cliente escribio.
///
/// Es el mismo para lo que no existe, lo que esta en cuarentena y una
/// etiqueta que no apunta a nada: el mensaje nunca nombra el digest
/// resuelto, que diria que el contenido esta ahi.
fn the_unknown_manifest(reference: &str) -> OciFailure {
    OciFailure::manifest_unknown(format!("the manifest {reference:?} is unknown"))
}

/// Lo que se responde cuando la compuerta de confianza no admite.
///
/// Lo no admitido responde con el mismo error OCI que lo que no existe
/// (`unknown`), para que la respuesta no diga que el contenido esta ahi.
/// Un libro que no responde es un 503: lo desconocido no se sirve.
fn the_admission_failure(
    admission: crate::trust::Admission,
    unknown: impl FnOnce(&str) -> OciFailure,
    digest: &str,
) -> OciFailure {
    match admission {
        crate::trust::Admission::Hidden => unknown(digest),
        crate::trust::Admission::Unavailable => {
            OciFailure::unavailable("the trust ledger is not available")
        }
    }
}

/// El tipo de medio que el manifiesto declara.
///
/// El manifiesto es la verdad de que es: si declara un tipo que este
/// registro no sirve, la peticion falla con el error OCI de manifiesto
/// desconocido, y no con un 200 que finge ser lo que el cliente pidio.
fn the_declared_media_type(bytes: &[u8]) -> Result<String, OciFailure> {
    let declared = serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|manifest| {
            manifest
                .get("mediaType")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        });
    match declared {
        Some(media_type) if SUPPORTED_MANIFESTS.contains(&media_type.as_str()) => Ok(media_type),
        // Un tipo que este registro NO sirve no se reetiqueta con otro:
        // servirlo con un tipo distinto es un 200 que finge ser lo que el
        // cliente pidio, y el cliente que lo pidio no lo recibio.
        Some(media_type) if hodei_hub::oci::UNSUPPORTED.contains(&media_type.as_str()) => Err(
            OciFailure::manifest_unknown(format!("the registry does not serve {media_type}")),
        ),
        // Un tipo que este registro no conoce: tampoco se sirve, por el
        // mismo motivo. Un `Content-Type` arbitrario en la respuesta es
        // ademas un problema de seguridad: un navegador que navegue a la
        // URL interpretaria el manifiesto como lo que declare.
        Some(media_type) => Err(OciFailure::manifest_unknown(format!(
            "the registry does not serve {media_type}"
        ))),
        // Un manifiesto sin `mediaType` no se sabe que es: el registro
        // sirve los tipos que conoce, y un manifiesto que no se
        // identifica no se sirve.
        None => Err(OciFailure::manifest_unknown(
            "the manifest does not declare its media type",
        )),
    }
}

/// La respuesta de bytes de la distribución: las cabeceras OCI y, en un
/// HEAD, nada de cuerpo.
fn oci_bytes_response(bytes: Vec<u8>, media_type: &str, digest: &str, head: bool) -> Response {
    let length = bytes.len().to_string();
    let mut response = if head {
        StatusCode::OK.into_response()
    } else {
        (StatusCode::OK, bytes).into_response()
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(media_type).expect("the media type is a header value"),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length).expect("the length is a header value"),
    );
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&format!("\"sha256:{digest}\""))
            .expect("the digest is a header value"),
    );
    headers.insert(
        "Docker-Content-Digest",
        HeaderValue::from_str(&format!("sha256:{digest}")).expect("the digest is a header value"),
    );
    // Un manifiesto es JSON. Sin esto, un navegador que navegue a la
    // URL interpretaria la respuesta como lo que el manifiesto declare.
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// La negociacion del `Accept`.
///
/// Un cliente que no nombra tipos (`*/*`, `application/*` o nada) acepta
/// lo que haya. Uno que solo nombra tipos que este registro no sirve
/// recibe el error OCI de manifiesto desconocido, no una respuesta que
/// finge ser lo que pidió.
///
/// El `Accept` se lee **entero**: un cliente puede mandar el tipo que
/// acepta en **varias cabeceras**, y cada cabecera puede traer una lista
/// separada por comas. Leer solo la primera cabecera--que es lo que hace
/// un `get`--deja fuera los tipos que el cliente sí acepta, y un cliente
/// de verdad (la CLI de Docker manda dos: la de indice y la de imagen)
/// se queda sin manifiesto.
///
/// Y el peso se respeta: un `q=0` dice que el tipo **no** se acepta
/// (RFC 9110, seccion 12.5.1), y un tipo rechazado es un tipo que no se
/// puede servir. Sin esto, un `Accept: */*;q=0`--que es un "no acepto
/// nada"--serviria cualquier manifiesto.
fn the_negotiation(headers: &HeaderMap, served: &str) -> Result<(), OciFailure> {
    let mut there_is_an_accept = false;
    let mut asked: Vec<&str> = Vec::new();
    for value in headers.get_all(header::ACCEPT).iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        there_is_an_accept = true;
        for part in value.split(',') {
            let mut pieces = part.split(';');
            let media = pieces.next().unwrap_or("").trim();
            if media.is_empty() {
                continue;
            }
            // El peso `q=0` es un rechazo: el tipo esta en la lista, pero
            // el cliente dice que no lo quiere.
            if pieces.any(the_weight_is_zero) {
                continue;
            }
            asked.push(media);
        }
    }
    if !there_is_an_accept {
        // Sin `Accept` el cliente acepta lo que haya: es lo que dice el
        // protocolo, y el tipo que se sirve lo declara el manifiesto.
        return Ok(());
    }
    if asked.is_empty() {
        // El cliente ha nombrado tipos y los ha rechazado **todos**: no
        // acepta ninguno. Servirle lo nuestro seria responder con algo
        // que ha dicho que no quiere.
        return Err(OciFailure::manifest_unknown(format!(
            "the client accepts none of the media types this registry serves: {served}"
        )));
    }
    // El cliente acepta cualquier cosa (`*/*`, `application/*`) o acepta
    // el tipo que se va a servir. Lo que NO se acepta es un cliente que
    // solo nombra otros tipos: servirle el nuestro seria responder con
    // algo que no pidio.
    let accepts = asked
        .iter()
        .any(|media| *media == "*/*" || *media == "application/*" || *media == served);
    if accepts {
        return Ok(());
    }
    Err(OciFailure::manifest_unknown(format!(
        "the registry does not serve any of the media types the client accepts: {}",
        asked.join(", ")
    )))
}

/// Si un parametro del `Accept` es un peso `q=0`.
///
/// El peso se escribe `q=0` o `q=0.0`: las dos formas dicen lo mismo, y
/// un peso que no se puede leer no es un rechazo.
fn the_weight_is_zero(parameter: &str) -> bool {
    let parameter = parameter.trim();
    let Some(weight) = parameter.strip_prefix("q=") else {
        return false;
    };
    weight
        .trim()
        .parse::<f64>()
        .is_ok_and(|weight| weight <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const THE_SERVED: &str = "application/vnd.oci.image.manifest.v1+json";

    /// Las cabeceras de una peticion con un `Accept`.
    fn with_accept(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(
                header::ACCEPT,
                HeaderValue::from_str(value).expect("a header value"),
            );
        }
        headers
    }

    /// Sin `Accept` se sirve lo que haya.
    #[test]
    fn without_accept_the_registry_serves_what_it_has() {
        assert!(the_negotiation(&HeaderMap::new(), THE_SERVED).is_ok());
    }

    /// El cliente que acepta cualquier cosa recibe lo que haya.
    #[test]
    fn a_client_that_accepts_anything_is_served() {
        assert!(the_negotiation(&with_accept(&["*/*"]), THE_SERVED).is_ok());
        assert!(the_negotiation(&with_accept(&["application/*"]), THE_SERVED).is_ok());
    }

    /// El cliente que nombra el tipo que se sirve recibe el manifiesto.
    #[test]
    fn a_client_that_accepts_the_served_type_is_served() {
        assert!(the_negotiation(&with_accept(&[THE_SERVED]), THE_SERVED).is_ok());
    }

    /// Un cliente que solo nombra otros tipos recibe el error del
    /// protocolo, no un manifiesto que no pidio.
    #[test]
    fn a_client_that_accepts_another_type_is_refused() {
        let headers = with_accept(&["application/vnd.oci.image.index.v1+json"]);
        assert!(the_negotiation(&headers, THE_SERVED).is_err());
    }

    /// La lista separada por comas se lee entera.
    #[test]
    fn a_comma_separated_list_is_read_whole() {
        let headers = with_accept(&["application/vnd.oci.image.index.v1+json, image/png"]);
        assert!(the_negotiation(&headers, "image/png").is_ok());
    }

    /// **Varias** cabeceras se leen enteras: la CLI de Docker manda la del
    /// indice y la de la imagen, y con una sola cabecera el cliente se
    /// queda sin manifiesto.
    #[test]
    fn several_accept_headers_are_read_whole() {
        let headers = with_accept(&["application/vnd.oci.image.index.v1+json", THE_SERVED]);
        assert!(the_negotiation(&headers, THE_SERVED).is_ok());
    }

    /// Un peso `q=0` es un rechazo: el tipo no se acepta.
    #[test]
    fn a_zero_weight_is_a_rejection() {
        let headers = with_accept(&["application/vnd.oci.image.index.v1+json;q=0"]);
        assert!(the_negotiation(&headers, THE_SERVED).is_err());
    }

    /// Y un `*/*` con peso cero no acepta cualquier cosa.
    #[test]
    fn a_zero_weight_on_everything_refuses_everything() {
        let headers = with_accept(&["*/*;q=0"]);
        assert!(the_negotiation(&headers, THE_SERVED).is_err());
    }

    /// Un peso que no es cero es una preferencia, no un rechazo.
    #[test]
    fn a_positive_weight_is_a_preference() {
        let headers = with_accept(&["application/vnd.oci.image.index.v1+json;q=0.8", THE_SERVED]);
        assert!(the_negotiation(&headers, THE_SERVED).is_ok());
    }

    /// Los parametros que no son el peso no pesan: el tipo sigue siendo el
    /// que se compara.
    #[test]
    fn a_parameter_is_not_a_weight() {
        let headers = with_accept(&["application/vnd.oci.image.manifest.v1+json;charset=utf-8"]);
        assert!(the_negotiation(&headers, THE_SERVED).is_ok());
    }
}
