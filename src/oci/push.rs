//! El push de OCI Distribution 1.1: las sesiones de subida y el manifiesto.
//!
//! El protocolo de push tiene dos piezas y una regla:
//!
//! 1. **Las sesiones de subida**: un `POST` abre una, un `PATCH` (o
//!    varios) la llenan, un `GET` dice cuanto lleva, un `PUT` la cierra
//!    con el digest y un `DELETE` la cancela.
//! 2. **El manifiesto**: un `PUT` a `/v2/<name>/manifests/<referencia>`
//!    publica los bytes del manifiesto y, si la referencia es una
//!    etiqueta, la etiqueta apunta a ellos.
//!
//! Y la regla es la que separa un registro de un almacen: **lo que se
//! publica es exactamente lo que el cliente dijo que era**. El digest
//! que el cliente declara se comprueba contra los bytes que han llegado,
//! y si no coinciden no se publica nada: ni el blob, ni el manifiesto, ni
//! la etiqueta.
//!
//! El contenido se escribe **por trozos** en el almacen de contenido, y
//! el commit es lo unico que publica: un blob a medio escribir nunca es
//! visible, ni para el que sube ni para el que descarga.

use axum::body::Body;
use axum::extract::OriginalUri;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use hodei_catalog::{PackageCoordinates, Publication};
use hodei_evidence::ContentDigest;
use hodei_hub::blob::{BlobError, BlobWriteSession, CommitExpectation};

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::oci::failure::OciFailure;
use crate::oci::target::{Target, is_a_digest, the_digest_of_reference, the_tag_name, the_target};
use crate::oci::{
    the_catalog, the_catalog_error, the_content_store, the_context, the_decision, the_repository,
};

/// Los tipos de medio de manifiesto que este registro publica.
///
/// Son los mismos que el pull sirve: el subconjunto que Hodei puede
/// verificar. Un indice multi-plataforma es de otra corte (HAR-08), y un
/// cliente que lo manda recibe un error claro en vez de una
/// reinterpretacion silenciosa.
const SUPPORTED_MANIFESTS: &[&str] = &[
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.v2+json",
];

/// La cabecera que anuncia la version de la API de distribucion.
const API_VERSION_HEADER: &str = "Docker-Distribution-API-Version";

/// La cabecera con el identificador de la sesion de subida.
///
/// No la pide el protocolo de OCI (es de Docker), pero la usan los
/// clientes que la conocen, y no cuesta nada llevarla.
const UPLOAD_UUID_HEADER: &str = "Docker-Upload-UUID";

/// La cabecera con el digest del contenido.
const CONTENT_DIGEST_HEADER: &str = "Docker-Content-Digest";

/// La cabecera con las etiquetas que el manifiesto acepta.
const OCI_TAG_HEADER: &str = "OCI-Tag";

/// Los parametros de la peticion que el protocolo usa.
#[derive(Debug, Default)]
struct TheQuery {
    /// El digest que el cliente declara.
    digest: Option<String>,
    /// El digest que el cliente quiere montar desde otro repositorio.
    mount: Option<String>,
    /// El repositorio de origen del montaje.
    from: Option<String>,
    /// Las etiquetas que el manifiesto acepta.
    tags: Vec<String>,
}

/// Los parametros de la peticion.
///
/// El query se lee a mano y no con un extractor: los parametros que
/// importan son pocos, y el que se repite (`tag`) tiene que repetirse
/// sin que un extractor se queje.
fn the_query(uri: &OriginalUri) -> TheQuery {
    let mut query = TheQuery::default();
    for (key, value) in url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes()) {
        match key.as_ref() {
            "digest" => query.digest = Some(value.into_owned()),
            "mount" => query.mount = Some(value.into_owned()),
            "from" => query.from = Some(value.into_owned()),
            "tag" => query.tags.push(value.into_owned()),
            _ => {}
        }
    }
    query
}

/// El punto de entrada de la superficie de escritura.
///
/// El metodo y el path dicen que operacion es, y cada operacion tiene sus
/// reglas. Lo que **no** hay es una operacion "generica": una peticion
/// que no sea una de las del protocolo se rechaza con el error del
/// protocolo, no con un 404 de router que confunde la ausencia con el
/// error.
pub async fn mutate(
    state: AppState,
    uri: OriginalUri,
    method: Method,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
) -> Result<Response, OciFailure> {
    match method {
        Method::POST => start_upload(state, uri, headers, request).await,
        Method::PATCH => append_chunk(state, uri, headers, request).await,
        Method::PUT => put(state, uri, headers, request).await,
        Method::GET => upload_status(state, uri, headers).await,
        Method::DELETE => cancel_upload(state, uri, headers).await,
        other => Err(OciFailure::unsupported(format!(
            "the distribution API does not have the method {other}"
        ))),
    }
}

/// `POST /v2/<name>/blobs/uploads/`
///
/// Abre una sesion de subida, o monta un blob que ya esta en otro
/// repositorio, o publica el blob entero si el cliente declara su digest.
async fn start_upload(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let target = the_target(uri.path())?;
    let Target::UploadStart { name } = target else {
        return Err(OciFailure::unsupported(
            "this path does not start an upload",
        ));
    };
    let repository = the_repository(&state, &context, &name).await?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;

    let query = the_query(&uri);

    // El montaje: el blob ya esta en otro repositorio de la MISMA cuenta.
    // Si no se puede montar, se cae a una subida normal: es lo que hace
    // un registro que no monta, y el cliente sube los bytes.
    if let Some(mount) = query.mount.as_deref()
        && let Some(response) = the_mount(
            &state,
            &context,
            &name,
            mount,
            query.from.as_deref(),
            &headers,
        )
        .await?
    {
        return Ok(response);
    }

    // La subida monolitica: el cuerpo entero, con su digest.
    if let Some(digest) = query.digest.as_deref() {
        return the_monolithic(
            &state,
            &context,
            &name,
            digest,
            request.into_body(),
            &headers,
        )
        .await;
    }

    // La sesion: el cliente recibe el `Location` y sigue escribiendo en
    // el.
    let store = the_content_store(&state)?;
    let writer = store
        .begin()
        .await
        .map_err(|error| the_blob_error(&error))?;
    let session = state
        .uploads
        .open(context.account().as_str(), &name, writer)
        .await?;
    let id = session.lock().await.the_id().to_string();
    let location = the_location_of_upload(&name, &id);

    let mut response = StatusCode::ACCEPTED.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&location).expect("the location is a header value"),
    );
    // Una subida que no tiene bytes todavia: el rango es `0-0`, que es
    // lo que dice el protocolo y lo que espera el cliente.
    headers.insert(header::RANGE, HeaderValue::from_static("0-0"));
    headers.insert(
        UPLOAD_UUID_HEADER,
        HeaderValue::from_str(&id).expect("the session id is a header value"),
    );
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(response)
}

/// El montaje de un blob que ya esta en otro repositorio.
///
/// Solo monta un blob de la **misma cuenta**: un repositorio de otra
/// cuenta no existe para esta, y un montaje desde el otro lado
/// copiaria bytes de una cuenta a otra. Y lo que no se puede montar no
/// se dice: la peticion cae a una subida normal, que es lo que hace un
/// registro que no monta.
async fn the_mount(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    mount: &str,
    from: Option<&str>,
    headers: &HeaderMap,
) -> Result<Option<Response>, OciFailure> {
    // El digest que se quiere montar tiene que ser un digest: un
    // montaje con un digest mal formado es un error del cliente, y no un
    // montaje que no se puede hacer.
    let digest = the_digest_of_reference(mount)?;

    // El repositorio de origen: el mismo, si el cliente no dice otro.
    let source = from.unwrap_or(name);
    let Ok(source_repository) = the_repository(state, context, source).await else {
        return Ok(None);
    };
    // Y el principal tiene que poder leer de el: un montaje es una
    // lectura de un repositorio y una escritura en otro.
    if the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &source_repository,
    )
    .is_err()
    {
        return Ok(None);
    }

    let (reader, _) = the_catalog(state)?;
    let asset = reader
        .asset(
            context.account().as_str(),
            source,
            &format!("blobs/{digest}"),
        )
        .await
        .ok();
    let Some(asset) = asset else {
        return Ok(None);
    };
    // Y los bytes tienen que estar en el almacen: una fila que apunta a
    // un blob que no esta es una fila rota, y montar un blob roto es
    // publicar un blob roto.
    let store = the_content_store(state)?;
    if !store.exists(&asset.digest).await.unwrap_or(false) {
        return Ok(None);
    }

    // El montaje no copia bytes: el CAS es un arbol comun y el contenido
    // ya esta. Lo que se escribe es la referencia del repositorio
    // destino, que es lo que hace que el blob sea de ese repositorio.
    publish_blob(
        state,
        context,
        name,
        &asset.digest,
        asset.size_bytes,
        headers,
    )
    .await?;

    let mut response = StatusCode::CREATED.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&the_location_of_blob(name, &digest))
            .expect("the location is a header value"),
    );
    headers.insert(
        CONTENT_DIGEST_HEADER,
        HeaderValue::from_str(&format!("sha256:{digest}")).expect("the digest is a header value"),
    );
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(Some(response))
}

/// La subida monolitica: el cuerpo entero y su digest.
///
/// El protocolo la define como un `POST` con `?digest=` y el contenido
/// entero. El servidor comprueba que lo que ha llegado es lo que el
/// cliente declaro, y si no, no publica nada.
async fn the_monolithic(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    declared: &str,
    body: Body,
    headers: &HeaderMap,
) -> Result<Response, OciFailure> {
    let declared = the_digest_of_reference(declared)?;
    let store = the_content_store(state)?;
    let mut writer = store
        .begin()
        .await
        .map_err(|error| the_blob_error(&error))?;
    let length = write_the_body(writer.as_mut(), body, state.max_body_size as u64).await?;

    let found = writer.the_digest();
    if found.value != declared {
        // El cliente dijo que mandaba otra cosa: se aborta la escritura
        // y no queda ni un byte en el almacen.
        let _ = writer.abort().await;
        return Err(OciFailure::digest_invalid(
            "the content does not match the declared digest",
        ));
    }
    let digest = writer
        .commit(CommitExpectation {
            digest: found.clone(),
            length,
        })
        .await
        .map_err(|error| the_blob_error(&error))?;

    publish_blob(state, context, name, &digest, length, headers).await?;

    let mut response = StatusCode::CREATED.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&the_location_of_blob(name, &digest.value))
            .expect("the location is a header value"),
    );
    headers.insert(
        CONTENT_DIGEST_HEADER,
        HeaderValue::from_str(&format!("sha256:{}", digest.value))
            .expect("the digest is a header value"),
    );
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(response)
}

/// `PATCH /v2/<name>/blobs/uploads/<uuid>`
///
/// Anade un trozo a la sesion.
async fn append_chunk(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let target = the_target(uri.path())?;
    let Target::UploadSession { name, session: id } = target else {
        return Err(OciFailure::unsupported(
            "this path does not carry an upload session",
        ));
    };
    let repository = the_repository(&state, &context, &name).await?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;

    let Some(shared) = state.uploads.find(context.account().as_str(), &name, &id) else {
        return Err(OciFailure::blob_upload_unknown(
            "the upload session does not exist",
        ));
    };
    let mut session = shared.lock().await;
    session.touch();
    let written = session.bytes_written();

    // El rango, si el cliente lo manda. El protocolo dice que los trozos
    // van en orden y que un trozo que no encaja recibe un 416: el
    // cliente recupera el offset con un `GET` y sigue.
    if let Some(range) = the_content_range(&headers)
        && range.0 != written
    {
        // El estado es el `416` que el protocolo dice para un rango
        // que no se puede satisfacer, y el `Range` de la respuesta
        // dice por donde seguir: el cliente recupera el offset y
        // continua.
        return Err(OciFailure::blob_upload_range_mismatch(format!(
            "the chunk starts at {} and the upload is at {written}",
            range.0
        ))
        .with_range(the_range_header(written)));
    }

    let length = write_the_body(
        session.the_writer().as_mut(),
        request.into_body(),
        state.max_body_size as u64,
    )
    .await?;

    Ok(the_upload_progress(&name, &id, written + length))
}

/// `GET /v2/<name>/blobs/uploads/<uuid>`
///
/// Dice cuanto lleva la sesion.
pub async fn upload_status(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let target = the_target(uri.path())?;
    let Target::UploadSession { name, session: id } = target else {
        return Err(OciFailure::unsupported(
            "this path does not carry an upload session",
        ));
    };
    let repository = the_repository(&state, &context, &name).await?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;

    let Some(shared) = state.uploads.find(context.account().as_str(), &name, &id) else {
        return Err(OciFailure::blob_upload_unknown(
            "the upload session does not exist",
        ));
    };
    let mut session = shared.lock().await;
    session.touch();
    let written = session.bytes_written();

    let mut response = StatusCode::NO_CONTENT.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&the_location_of_upload(&name, &id))
            .expect("the location is a header value"),
    );
    headers.insert(
        header::RANGE,
        HeaderValue::from_str(&the_range_header(written)).expect("the range is a header value"),
    );
    headers.insert(
        UPLOAD_UUID_HEADER,
        HeaderValue::from_str(&id).expect("the session id is a header value"),
    );
    Ok(response)
}

/// `DELETE /v2/<name>/blobs/uploads/<uuid>`
///
/// Cancela la sesion: lo que se habia escrito no se publica nunca.
async fn cancel_upload(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let target = the_target(uri.path())?;
    let Target::UploadSession { name, session: id } = target else {
        return Err(OciFailure::unsupported(
            "this path does not carry an upload session",
        ));
    };
    let repository = the_repository(&state, &context, &name).await?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;

    let Some(shared) = state.uploads.find(context.account().as_str(), &name, &id) else {
        return Err(OciFailure::blob_upload_unknown(
            "the upload session does not exist",
        ));
    };
    // La sesion se aborta antes de salir del registro: al soltarse, su
    // escritura del CAS se cierra y el staging se borra.
    {
        let mut session = shared.lock().await;
        let _ = session.the_writer().abort().await;
    }
    state.uploads.remove(&id);

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `PUT`: o se cierra una sesion de subida, o se publica un manifiesto.
async fn put(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
) -> Result<Response, OciFailure> {
    let target = the_target(uri.path())?;
    match target {
        Target::UploadSession { name, session } => {
            close_upload(state, uri, headers, request, name, session).await
        }
        Target::Manifest { name, reference } => {
            put_manifest(state, uri, headers, request, name, reference).await
        }
        _ => Err(OciFailure::unsupported("this path cannot be written to")),
    }
}

/// `PUT /v2/<name>/blobs/uploads/<uuid>?digest=<digest>`
///
/// Cierra la sesion: comprueba el digest, publica el blob y devuelve el
/// sitio donde esta.
async fn close_upload(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
    name: String,
    id: String,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let repository = the_repository(&state, &context, &name).await?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;

    let query = the_query(&uri);
    let Some(declared) = query.digest.as_deref() else {
        return Err(OciFailure::digest_invalid(
            "the closing request must carry the digest of the whole blob",
        ));
    };
    let declared = the_digest_of_reference(declared)?;

    let Some(shared) = state.uploads.find(context.account().as_str(), &name, &id) else {
        return Err(OciFailure::blob_upload_unknown(
            "the upload session does not exist",
        ));
    };
    let mut session = shared.lock().await;
    session.touch();

    // Lo que ya lleva la sesion: el `PUT` que cierra puede no traer
    // cuerpo (es lo que hacen los clientes que suben por trozos), y lo
    // que se declara es el **blob entero**, no el ultimo trozo.
    let already = session.bytes_written();
    // El ultimo trozo puede venir en el `PUT` que cierra.
    let last = write_the_body(
        session.the_writer().as_mut(),
        request.into_body(),
        state.max_body_size as u64,
    )
    .await?;
    let length = already + last;

    let found = session.the_writer().the_digest();
    if found.value != declared {
        // El digest no cuadra: la subida se descarta y no queda ninguna
        // referencia visible. Un blob con un digest que no es el suyo no
        // es un blob.
        let _ = session.the_writer().abort().await;
        drop(session);
        state.uploads.remove(&id);
        return Err(OciFailure::digest_invalid(
            "the content does not match the declared digest",
        ));
    }
    let digest = session
        .the_writer()
        .commit(CommitExpectation {
            digest: found.clone(),
            length,
        })
        .await
        .map_err(|error| the_blob_error(&error))?;
    drop(session);
    state.uploads.remove(&id);

    publish_blob(&state, &context, &name, &digest, length, &headers).await?;

    let mut response = StatusCode::CREATED.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&the_location_of_blob(&name, &digest.value))
            .expect("the location is a header value"),
    );
    headers.insert(
        CONTENT_DIGEST_HEADER,
        HeaderValue::from_str(&format!("sha256:{}", digest.value))
            .expect("the digest is a header value"),
    );
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(response)
}

/// `PUT /v2/<name>/manifests/<referencia>`
///
/// Publica el manifiesto.
///
/// Lo que se guarda son **los bytes exactos** que envio el cliente: el
/// manifiesto es la verdad de que es, y un manifiesto reescrito es un
/// manifiesto con otro digest.
async fn put_manifest(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
    name: String,
    reference: String,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let repository = the_repository(&state, &context, &name).await?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;

    // Los bytes del manifiesto, tal cual.
    let body = read_the_body(request.into_body(), state.max_body_size as u64).await?;

    // El tipo de medio lo declara la cabecera, y tiene que ser del
    // subconjunto que este registro publica.
    let declared_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or(value).trim())
        .unwrap_or_default()
        .to_string();
    if declared_type.is_empty() {
        return Err(OciFailure::manifest_invalid(
            "the manifest must declare its media type",
        ));
    }
    if !SUPPORTED_MANIFESTS.contains(&declared_type.as_str()) {
        return Err(OciFailure::manifest_invalid(format!(
            "the registry does not publish manifests of type {declared_type}"
        )));
    }

    // Y la cabecera tiene que coincidir con el `mediaType` del cuerpo:
    // el manifiesto dice de que es, y la cabecera es lo que el cliente
    // pidio.
    let manifest = serde_json::from_slice::<serde_json::Value>(&body).map_err(|error| {
        OciFailure::manifest_invalid(format!("the manifest is not readable: {error}"))
    })?;
    match manifest.get("mediaType").and_then(|value| value.as_str()) {
        Some(declared) if declared != declared_type => {
            return Err(OciFailure::manifest_invalid(format!(
                "the manifest declares {declared} and the request declares {declared_type}"
            )));
        }
        None => {
            return Err(OciFailure::manifest_invalid(
                "the manifest does not declare its media type",
            ));
        }
        _ => {}
    }

    // El digest: si la referencia es un digest, tiene que ser el de los
    // bytes que han llegado. Si no, no se publica nada y la etiqueta no
    // se crea.
    let digest = ContentDigest::of_bytes(&body);
    if is_a_digest(&reference) {
        let expected = the_digest_of_reference(&reference)?;
        if expected != digest.value {
            return Err(OciFailure::digest_invalid(format!(
                "the manifest does not match the digest {reference}"
            )));
        }
    }

    // Las referencias exigidas: la configuracion y las capas tienen que
    // estar en el repositorio. Un manifiesto que apunta a algo que no
    // esta es un manifiesto que no se puede descargar.
    let required = the_required_digests(&manifest)?;
    let (reader, _) = the_catalog(&state)?;
    for required in &required {
        let asset = reader
            .asset(
                context.account().as_str(),
                &name,
                &format!("blobs/{required}"),
            )
            .await;
        if asset.is_err() {
            return Err(OciFailure::manifest_blob_unknown(format!(
                "the repository does not have the blob {required}"
            )));
        }
    }

    // Los bytes, tal cual.
    let store = the_content_store(&state)?;
    store
        .put(&body)
        .await
        .map_err(|error| the_blob_error(&error))?;

    // La etiqueta: la referencia es una etiqueta, o el cliente pide
    // etiquetas con `?tag=`. Las dos cosas pueden a la vez.
    let mut tags = Vec::new();
    if !is_a_digest(&reference) {
        tags.push(reference.clone());
    }
    tags.extend(the_query(&uri).tags);
    for tag in &tags {
        the_tag_name(tag)?;
    }

    let asset_path = format!("manifests/{}", digest.value);
    let coordinates = PackageCoordinates {
        package_name: asset_path.clone(),
        package_version: digest.value.clone(),
    };
    let publication = Publication {
        repository: name.clone(),
        asset_path: asset_path.clone(),
        coordinates: coordinates.clone(),
        digest: digest.clone(),
        size_bytes: body.len() as u64,
        // Las etiquetas **no** van en la publicacion: una etiqueta es
        // mutable en el protocolo, y la publicacion crea la etiqueta
        // una vez. Reetiquetar es moverla, que es otra operacion.
        tags: Vec::new(),
        actor: context.principal().subject().as_str().to_string(),
        correlation_id: correlation_id(&headers, &state.identity),
        // La sustitucion la decide el **repositorio**, no el cliente: un
        // manifiesto publicado no se reescribe, y lo que hay detras de
        // una etiqueta que se mueve sigue declarado con su digest.
        overwrite: true,
        precondition: None,
    };
    let (_, writer) = the_catalog(&state)?;
    writer
        .publish(context.account().as_str(), publication)
        .await
        .map_err(|error| the_catalog_error(&error))?;

    // Las etiquetas se **mueven**. El protocolo dice que una etiqueta es
    // mutable: publicar un manifiesto con el nombre de una etiqueta que
    // ya existe repointa la etiqueta, y lo que hay detras de la
    // anterior sigue publicado con su digest.
    //
    // Es lo que distingue a un registro de un almacen: en un almacen el
    // nombre es la cosa, y en un registro la etiqueta es una referencia
    // que se puede mover. Moverla no toca los bytes de ninguno de los
    // dos manifiestos.
    for tag in &tags {
        writer
            .move_tag(context.account().as_str(), &name, tag, coordinates.clone())
            .await
            .map_err(|error| the_catalog_error(&error))?;
    }

    let correlation = correlation_id(&headers, &state.identity);
    record_audit(
        &state,
        context.principal().subject().as_str(),
        context.account().as_str(),
        "publish",
        &format!("{name}/manifests/{}", digest.value),
        AuditResult::Allowed,
        &correlation,
    );

    let mut response = StatusCode::CREATED.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&the_location_of_manifest(&name, &digest.value))
            .expect("the location is a header value"),
    );
    headers.insert(
        CONTENT_DIGEST_HEADER,
        HeaderValue::from_str(&format!("sha256:{}", digest.value))
            .expect("the digest is a header value"),
    );
    // Las etiquetas que el manifiesto ha aceptado: el cliente las pide
    // con `?tag=` y el registro las dice una a una.
    for tag in &tags {
        if let Ok(value) = HeaderValue::from_str(tag) {
            headers.append(OCI_TAG_HEADER, value);
        }
    }
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(response)
}

/// Los digests que el manifiesto exige que esten en el repositorio.
///
/// La configuracion y las capas. Un manifiesto sin configuracion no es
/// un manifiesto de imagen, y un manifiesto que no se puede leer no se
/// publica.
///
/// El digest se devuelve **sin el algoritmo**, que es como lo nombra el
/// catalogo: un blob vive en `blobs/<digest>`, y el prefijo `sha256:`
/// pertenece a la referencia OCI, no a la ruta del asset. Un digest que
/// no lo lleva no es un digest, y un manifiesto que lo nombra no se
/// puede comprobar.
fn the_required_digests(manifest: &serde_json::Value) -> Result<Vec<String>, OciFailure> {
    let mut required = Vec::new();
    let config = manifest
        .get("config")
        .and_then(|config| config.get("digest"))
        .and_then(|digest| digest.as_str())
        .ok_or_else(|| OciFailure::manifest_invalid("the manifest does not name a config"))?;
    required.push(the_hexadecimal_digest(config)?);
    if let Some(layers) = manifest.get("layers").and_then(|layers| layers.as_array()) {
        for layer in layers {
            let Some(digest) = layer.get("digest").and_then(|digest| digest.as_str()) else {
                return Err(OciFailure::manifest_invalid(
                    "a layer of the manifest does not name a digest",
                ));
            };
            required.push(the_hexadecimal_digest(digest)?);
        }
    }
    Ok(required)
}

/// El hexadecimal de un digest, sin su algoritmo.
///
/// Un digest sin el prefijo `sha256:` no es un digest: el manifiesto
/// tiene que nombrar el contenido de la forma que lo nombra el
/// protocolo, y un digest a medio escribir no se puede comprobar.
fn the_hexadecimal_digest(digest: &str) -> Result<String, OciFailure> {
    let Some((algorithm, hexadecimal)) = digest.split_once(':') else {
        return Err(OciFailure::manifest_invalid(format!(
            "the digest {digest:?} does not carry its algorithm"
        )));
    };
    if algorithm != "sha256"
        || hexadecimal.is_empty()
        || !hexadecimal.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(OciFailure::manifest_invalid(format!(
            "the digest {digest:?} is not one this registry understands"
        )));
    }
    Ok(hexadecimal.to_string())
}

/// Escribe el cuerpo en la sesion de escritura.
///
/// Los bytes van **por trozos**, y el digest se calcula mientras pasan:
/// el cliente puede declarar cual va a ser, y si no declara ninguno, el
/// servidor lo calcula sobre lo que llega. En los dos casos, el commit
/// comprueba que lo publicado es exactamente lo declarado.
async fn write_the_body(
    writer: &mut dyn BlobWriteSession,
    body: Body,
    limit: u64,
) -> Result<u64, OciFailure> {
    let mut stream = body.into_data_stream();
    let mut length: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| OciFailure::blob_upload_invalid("the body could not be read"))?;
        length = length.saturating_add(chunk.len() as u64);
        if length > limit {
            // El contenido es mas grande de lo que se admite: se
            // aborta la escritura y se dice, en vez de aceptarlo y
            // fallar mas tarde por disco.
            let _ = writer.abort().await;
            return Err(OciFailure::size_invalid(format!(
                "the content is larger than the {limit} bytes this service accepts"
            )));
        }
        writer
            .append(&chunk)
            .await
            .map_err(|error| the_blob_error(&error))?;
    }
    Ok(length)
}

/// Lee el cuerpo entero.
///
/// El manifiesto es un documento: se lee entero para poder compararlo con
/// su tipo de medio y para saber que digests exige. El limite es el
/// mismo que el de cualquier otra escritura.
async fn read_the_body(body: Body, limit: u64) -> Result<Vec<u8>, OciFailure> {
    let mut bytes = Vec::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| OciFailure::manifest_invalid("the body could not be read"))?;
        bytes.extend_from_slice(&chunk);
        if bytes.len() as u64 > limit {
            return Err(OciFailure::size_invalid(format!(
                "the manifest is larger than the {limit} bytes this service accepts"
            )));
        }
    }
    Ok(bytes)
}

/// El rango que el cliente declara en el `Content-Range`.
///
/// El protocolo dice que el rango es `inicio-fin`, con los dos
/// extremos incluidos, y que el primer trozo tiene que empezar en 0.
fn the_content_range(headers: &HeaderMap) -> Option<(u64, u64)> {
    let value = headers
        .get("content-range")
        .and_then(|value| value.to_str().ok())?
        .trim();
    let (start, end) = value.split_once('-')?;
    Some((start.trim().parse().ok()?, end.trim().parse().ok()?))
}

/// El `Range` que dice cuanto lleva la subida.
///
/// El protocolo lo expresa como `0-<ultimo byte recibido>`, y una
/// subida que no tiene bytes se dice `0-0`.
fn the_range_header(written: u64) -> String {
    if written == 0 {
        "0-0".to_string()
    } else {
        format!("0-{}", written - 1)
    }
}

/// La respuesta de una subida que sigue en curso.
fn the_upload_progress(name: &str, id: &str, written: u64) -> Response {
    let mut response = StatusCode::ACCEPTED.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&the_location_of_upload(name, id))
            .expect("the location is a header value"),
    );
    headers.insert(
        header::RANGE,
        HeaderValue::from_str(&the_range_header(written)).expect("the range is a header value"),
    );
    headers.insert(
        UPLOAD_UUID_HEADER,
        HeaderValue::from_str(id).expect("the session id is a header value"),
    );
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    response
}

/// El sitio de un blob.
fn the_location_of_blob(name: &str, digest: &str) -> String {
    format!("/v2/{name}/blobs/sha256:{digest}")
}

/// El sitio de un manifiesto.
fn the_location_of_manifest(name: &str, digest: &str) -> String {
    format!("/v2/{name}/manifests/sha256:{digest}")
}

/// El sitio de una sesion de subida.
fn the_location_of_upload(name: &str, session: &str) -> String {
    format!("/v2/{name}/blobs/uploads/{session}")
}

/// Publica un blob como asset del repositorio.
///
/// El blob es un asset en `blobs/<digest>`: la fila es lo que hace que
/// los bytes sean **de este repositorio**, y lo que autoriza a servirlos.
async fn publish_blob(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    digest: &ContentDigest,
    size_bytes: u64,
    headers: &HeaderMap,
) -> Result<(), OciFailure> {
    let asset_path = format!("blobs/{}", digest.value);
    let publication = Publication {
        repository: name.to_string(),
        asset_path: asset_path.clone(),
        coordinates: PackageCoordinates {
            package_name: asset_path,
            package_version: digest.value.clone(),
        },
        digest: digest.clone(),
        size_bytes,
        tags: Vec::new(),
        actor: context.principal().subject().as_str().to_string(),
        correlation_id: correlation_id(headers, &state.identity),
        // El camino lleva el digest: republicar el mismo blob es lo
        // mismo, y no hay nada que sustituir.
        overwrite: true,
        precondition: None,
    };
    let (_, writer) = the_catalog(state)?;
    writer
        .publish(context.account().as_str(), publication)
        .await
        .map_err(|error| the_catalog_error(&error))?;
    Ok(())
}

/// Un error del almacen de contenido, traducido al dialecto.
fn the_blob_error(error: &BlobError) -> OciFailure {
    match error {
        BlobError::NotFound => OciFailure::blob_unknown("the content is not there"),
        // El contenido no es el que se declaro: es un error del
        // cliente, y lo dice el codigo del cliente.
        BlobError::DigestMismatch { .. } => {
            OciFailure::digest_invalid("the content does not match the declared digest")
        }
        BlobError::LengthMismatch { .. } => {
            OciFailure::size_invalid("the content does not have the declared length")
        }
        // El almacen esta roto: no es culpa del cliente.
        BlobError::Corrupted { .. } => {
            OciFailure::unavailable("the content store is not available")
        }
        BlobError::UnsupportedAlgorithm(_) => {
            OciFailure::digest_invalid("the registry does not compute digests with that algorithm")
        }
        BlobError::SessionClosed => OciFailure::blob_upload_invalid("the upload session is closed"),
        BlobError::Unavailable(_) => OciFailure::unavailable("the content store is not available"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El rango que el cliente declara se lee con sus dos extremos.
    #[test]
    fn the_content_range_is_read_with_both_ends() {
        let mut headers = HeaderMap::new();
        headers.insert("content-range", "0-99".parse().expect("a header"));
        assert_eq!(the_content_range(&headers), Some((0, 99)));

        headers.insert("content-range", "100-199".parse().expect("a header"));
        assert_eq!(the_content_range(&headers), Some((100, 199)));

        headers.insert("content-range", "not-a-range".parse().expect("a header"));
        assert_eq!(the_content_range(&headers), None);
    }

    /// El rango que dice la subida es `0-<ultimo byte>`.
    #[test]
    fn the_range_of_the_upload_is_the_last_byte() {
        assert_eq!(the_range_header(0), "0-0");
        assert_eq!(the_range_header(1), "0-0");
        assert_eq!(the_range_header(100), "0-99");
    }

    /// Las referencias exigidas de un manifiesto son su config y sus
    /// capas, y nada mas.
    ///
    /// El digest se queda **sin el algoritmo**: es como lo nombra el
    /// catalogo, que guarda el blob en `blobs/<digest>`.
    #[test]
    fn the_required_digests_are_the_config_and_the_layers() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": { "digest": "sha256:aaa" },
            "layers": [{ "digest": "sha256:bbb" }, { "digest": "sha256:ccc" }],
        });
        let required = the_required_digests(&manifest).expect("the digests");
        assert_eq!(required, vec!["aaa", "bbb", "ccc"]);
    }

    /// Un digest sin su algoritmo no es un digest, y un manifiesto que lo
    /// nombra no se puede comprobar.
    #[test]
    fn a_required_digest_without_its_algorithm_is_refused() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": { "digest": "aaa" },
            "layers": [],
        });
        let error = the_required_digests(&manifest).expect_err("there is no algorithm");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");

        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": { "digest": "md5:aaa" },
            "layers": [],
        });
        let error = the_required_digests(&manifest).expect_err("the algorithm is not sha256");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");
    }

    /// Un manifiesto sin configuracion no es un manifiesto de imagen.
    #[test]
    fn a_manifest_without_a_config_is_not_publishable() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "layers": [],
        });
        let error = the_required_digests(&manifest).expect_err("there is no config");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");
    }
}
