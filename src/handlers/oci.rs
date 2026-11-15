//! The OCI Distribution pull surface (HSP-100 / HAR-06), on the wire.
//!
//! `hodei-hub::oci` reads manifests as **types**; this is the **wire**:
//! the endpoints `/v2` of the distribution spec, served from the catalog
//! and the blob store that already exist. There is no image storage of
//! its own: a manifest is an asset of the repository, a blob is an asset
//! the repository references, and a tag is the mutable reference the
//! catalog resolves. The slice is **pull**: `GET /v2/`, blobs and
//! manifests by digest and by tag. The push is not announced yet.
//!
//! The identity rules are the raw surface's: the account comes from the
//! verified principal, and a repository of another account does not
//! exist for this one. The errors speak **OCI**: an envelope of codes
//! (BLOB_UNKNOWN, MANIFEST_UNKNOWN, NAME_UNKNOWN, DIGEST_INVALID,
//! NAME_INVALID), never a bare 404 that confuses "you cannot see it"
//! with "it is not there".

use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hodei_catalog::{CatalogReader, CatalogWriter};

use crate::app_state::AppState;

/// Los media types de manifiesto que este registro sirve.
const SUPPORTED_MANIFESTS: &[&str] = &[
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.v2+json",
];

/// La cabecera que anuncia la versión de la API de distribución.
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

/// El contenido de la distribución: blobs y manifests.
///
/// El GET y el HEAD comparten handler: el HEAD dice las mismas cabeceras
/// y no trae cuerpo, que es lo que el spec pide y lo que un cliente de
/// pull necesita para saber el tamaño sin descargarlo.
pub async fn content(
    State(state): State<AppState>,
    uri: OriginalUri,
    headers: HeaderMap,
    method: axum::http::Method,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let head = method == axum::http::Method::HEAD;

    // El nombre, la clase de recurso y la referencia salen del path en
    // crudo: `/<name>/blobs/<digest>` o `/<name>/manifests/<reference>`,
    // donde `<name>` puede tener niveles (`team/demo`).
    // El prefijo `/v2` se quita del principio: lo que queda es
    // `<name>/<kind>/<reference>`, y el nombre es lo que va antes de la
    // clase de recurso.
    let target = uri
        .path()
        .strip_prefix("/v2/")
        .ok_or_else(|| OciFailure::name_invalid("the path is not an OCI resource path"))?;
    let mut segments: Vec<&str> = target.split('/').collect();
    if segments.len() < 3 {
        return Err(OciFailure::name_invalid(
            "the path does not name a repository, a kind and a reference",
        ));
    }
    let reference = segments.pop().expect("the reference is there");
    let kind = segments.pop().expect("the kind is there");
    let name = segments.join("/");

    // El nombre se valida ANTES de mirar el catálogo: un `..` no llega a
    // ser una consulta, y un nombre mal formado es un error del cliente.
    the_name(&name)?;

    match kind {
        "blobs" => blob(&state, &context, &name, reference, head).await,
        "manifests" => manifest(&state, &context, &name, reference, &headers, head).await,
        other => Err(OciFailure::unsupported(format!(
            "the resource kind {other:?} is not part of the distribution API"
        ))),
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
            other => OciFailure::unavailable(other.to_string()),
        })?;

    let store = the_content_store(state)?;
    let bytes = store
        .get(&hodei_evidence::ContentDigest::sha256(digest.clone()))
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
                hodei_catalog::CatalogError::Missing { .. } => OciFailure::manifest_unknown(
                    format!("the tag {reference:?} does not point at a manifest"),
                ),
                other => OciFailure::unavailable(other.to_string()),
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
            hodei_catalog::CatalogError::Missing { .. } => {
                OciFailure::manifest_unknown(format!("there is no manifest {digest}"))
            }
            other => OciFailure::unavailable(other.to_string()),
        })?;

    // La negociación: si el cliente nombra tipos y ninguno es uno que
    // este registro sirve, el error es de manifiesto desconocido y no un
    // 200 engañoso.
    the_negotiation(headers)?;

    let store = the_content_store(state)?;
    let bytes = store
        .get(&hodei_evidence::ContentDigest::sha256(digest.clone()))
        .await
        .map_err(|_| {
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
    let media_type = the_declared_media_type(&bytes);
    debug_assert_eq!(
        bytes.len() as u64,
        asset.size_bytes,
        "the catalog and the store disagree about the size of the manifest"
    );

    Ok(oci_bytes_response(bytes, &media_type, &digest, head))
}

/// El tipo de medio que el manifiesto declara.
///
/// El manifiesto es la verdad de que es: si declara un tipo que este
/// registro no sirve, la peticion falla con el error OCI de manifiesto
/// desconocido, y no con un 200 que finge ser lo que el cliente pidio.
fn the_declared_media_type(bytes: &[u8]) -> String {
    let declared = serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|manifest| {
            manifest
                .get("mediaType")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        });
    match declared {
        Some(media_type) if SUPPORTED_MANIFESTS.contains(&media_type.as_str()) => media_type,
        Some(media_type) if hodei_hub::oci::UNSUPPORTED.contains(&media_type.as_str()) => {
            // Un tipo que este registro no sirve: la negociacion ya lo
            // ha rechazado antes de llegar aqui, y si se llega, es que el
            // cliente no ha limitado los tipos.
            hodei_hub::oci::MANIFEST_MEDIA_TYPE.to_string()
        }
        Some(media_type) => media_type,
        None => hodei_hub::oci::MANIFEST_MEDIA_TYPE.to_string(),
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
    response
}

/// El principal verificado de la petición.
///
/// La autenticación es la primera puerta, igual que en la superficie
/// cruda: una petición sin credencial no llega a mirar la ruta.
async fn the_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<hodei_authz::VerifiedContext, OciFailure> {
    let token = crate::handlers::bearer_token(headers).ok_or_else(OciFailure::unauthorized)?;
    let principal = match hodei_authz::verify_principal(
        state.authenticator.as_ref(),
        &hodei_authz::Credentials::bearer(token),
    )
    .await
    {
        Ok(principal) => principal,
        Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
            return Err(OciFailure::unavailable(
                "the authentication provider is not available",
            ));
        }
        Err(_) => return Err(OciFailure::unauthorized()),
    };
    Ok(hodei_authz::VerifiedContext::from_principal(principal))
}

/// El catálogo, con lo que el necesita para leer.
type TheCatalog<'a> = (
    &'a std::sync::Arc<dyn CatalogReader>,
    &'a std::sync::Arc<dyn CatalogWriter>,
);

/// El catálogo.
fn the_catalog(state: &AppState) -> Result<TheCatalog<'_>, OciFailure> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    let writer = state
        .catalog_writer
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    Ok((reader, writer))
}

/// El almacén de contenido.
fn the_content_store(
    state: &AppState,
) -> Result<&std::sync::Arc<dyn hodei_hub::blob::BlobStore>, OciFailure> {
    state
        .content_store
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the content store is not wired"))
}

/// El repositorio, en la cuenta verificada.
///
/// Un repositorio de otra cuenta no existe para esta, y se responde con
/// el `NAME_UNKNOWN` de la distribución: no dice si existe en otro sitio.
async fn the_repository(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
) -> Result<hodei_catalog::RepositoryDescriptor, OciFailure> {
    let (reader, _) = the_catalog(state)?;
    match reader.repository(context.account().as_str(), name).await {
        Ok(descriptor) => Ok(descriptor),
        Err(hodei_catalog::CatalogError::Missing { .. })
        | Err(hodei_catalog::CatalogError::Denied { .. }) => Err(OciFailure::name_unknown(
            format!("the repository {name:?} is not known to this account"),
        )),
        Err(other) => Err(OciFailure::unavailable(other.to_string())),
    }
}

/// La decisión de Cedar sobre el repositorio.
///
/// El recurso es el repositorio, igual que en la superficie cruda: una
/// acción sobre un recurso que no existe no se puede evaluar, y la
/// decisión la lleva el gate con el recurso a la vista.
fn the_decision(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    action: hodei_authz::model::ActionId,
    repository: &hodei_catalog::RepositoryDescriptor,
) -> Result<(), OciFailure> {
    let outcome = state
        .authorize_resource
        .execute_canonical(context.clone(), action, &repository.hrn)
        .map_err(|_| OciFailure::unavailable("the authorization engine is not available"))?;
    if !outcome.allows_access() {
        return Err(OciFailure::denied(
            "the principal is not allowed to pull from this repository",
        ));
    }
    Ok(())
}

/// El nombre del repositorio, con sus reglas.
///
/// Un nombre OCI es minúsculas, dígitos y separadores, con niveles
/// separados por barra. Un `..` no es un nivel: es un traversal, y se
/// rechaza antes de mirar el catálogo.
fn the_name(name: &str) -> Result<(), OciFailure> {
    if name.is_empty() || name.len() > 255 {
        return Err(OciFailure::name_invalid(
            "the repository name is empty or too long",
        ));
    }
    if name.starts_with('/') || name.ends_with('/') {
        return Err(OciFailure::name_invalid(
            "the repository name does not start or end with a separator",
        ));
    }
    for segment in name.split('/') {
        if segment.is_empty() {
            return Err(OciFailure::name_invalid(
                "the repository name has an empty level",
            ));
        }
        if segment == "." || segment == ".." {
            return Err(OciFailure::name_invalid(
                "the repository name does not carry navigation levels",
            ));
        }
        if !segment
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
        {
            return Err(OciFailure::name_invalid(
                "the repository name only admits lowercase letters, digits, '.', '_' and '-'",
            ));
        }
    }
    Ok(())
}

/// El digest que la referencia nombra.
///
/// Una referencia de blob o de manifest por digest lleva el algoritmo y
/// el hexadecimal (`sha256:<64 hex>`). Sin el prefijo, la referencia es
/// una **etiqueta**, que es mutable: el digest pelado no se acepta como
/// digest, porque un cliente que cree que pide por digest y recibe
/// contenido mutable se lleva un manifiesto que puede haber cambiado
/// entre la peticion y el uso.
///
/// Lo que no cuadre es un error del cliente, y el codigo OCI lo dice.
fn the_digest_of_reference(reference: &str) -> Result<String, OciFailure> {
    if !reference.starts_with("sha256:") {
        return Err(OciFailure::digest_invalid(format!(
            "{reference:?} does not name a digest: a digest carries its algorithm"
        )));
    }
    let digest = hodei_hub::oci::the_digest_of(reference)
        .ok_or_else(|| {
            OciFailure::digest_invalid(format!(
                "{reference:?} is not a content digest the registry understands"
            ))
        })?
        .value;
    Ok(digest)
}

/// La negociación del `Accept`.
///
/// Un cliente que no nombra tipos (`*/*`, `application/*` o nada) acepta
/// lo que haya. Uno que solo nombra tipos que este registro no sirve
/// recibe el error OCI de manifiesto desconocido, no una respuesta que
/// finge ser lo que pidió.
fn the_negotiation(headers: &HeaderMap) -> Result<(), OciFailure> {
    let Some(accept) = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(());
    };
    let asked: Vec<&str> = accept.split(',').map(|part| part.trim()).collect();
    if asked.iter().any(|media| {
        *media == "*/*" || *media == "application/*" || SUPPORTED_MANIFESTS.contains(media)
    }) {
        return Ok(());
    }
    Err(OciFailure::manifest_unknown(format!(
        "the registry does not serve any of the media types the client accepts: {accept}"
    )))
}

/// Por qué una petición OCI no se puede servir, con su código OCI.
#[derive(Debug, Clone)]
pub struct OciFailure {
    /// El código del error en el dialecto de la distribución.
    code: &'static str,
    /// Lo que pasó, dicho sin credenciales ni rutas de disco.
    message: String,
    /// El estado HTTP que el código OCI lleva.
    status: StatusCode,
}

impl OciFailure {
    fn blob_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "BLOB_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
        }
    }
    fn manifest_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "MANIFEST_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
        }
    }
    fn name_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "NAME_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
        }
    }
    fn name_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "NAME_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
        }
    }
    fn digest_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "DIGEST_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
        }
    }
    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            code: "UNSUPPORTED",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
        }
    }
    fn denied(message: impl Into<String>) -> Self {
        Self {
            code: "DENIED",
            message: message.into(),
            status: StatusCode::FORBIDDEN,
        }
    }
    fn unauthorized() -> Self {
        Self {
            code: "UNAUTHORIZED",
            message: "authentication required".into(),
            status: StatusCode::UNAUTHORIZED,
        }
    }
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "UNAVAILABLE",
            message: message.into(),
            status: StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

/// La respuesta de error, en el dialecto de la distribución.
impl IntoResponse for OciFailure {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "errors": [{
                "code": self.code,
                "message": self.message,
            }]
        });
        (
            self.status,
            [(header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        )
            .into_response()
    }
}
