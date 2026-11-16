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
use hodei_hub::oci::ManifestKind;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::oci::failure::OciFailure;
use crate::oci::target::{Target, is_a_digest, the_digest_of_reference, the_tag_name, the_target};
use crate::oci::{
    the_catalog, the_catalog_error, the_content_store, the_context, the_decision, the_repository,
};

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

/// La cabecera con el `subject` del manifiesto.
///
/// El protocolo dice que la respuesta al push **puede** llevar el
/// `subject` del manifiesto publicado, y lo lleva: es lo que le dice al
/// cliente a que manifiesto se ha colgado lo que ha subido.
const OCI_SUBJECT_HEADER: &str = "OCI-Subject";

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

    // Montar es leer del origen: si el origen es un canal trusted, el
    // digest tiene que estar concedido alli. Si no, el blob en cuarentena
    // saldria a un repositorio abierto sin promoverse nunca. Lo no
    // admitido cae a una subida normal, igual que lo que no existe: ni el
    // 201 ni el 202 dicen si el digest esta en cuarentena.
    if crate::trust::admit(
        state,
        context.account().as_str(),
        &source_repository,
        &asset.digest,
    )
    .await
    .is_err()
    {
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

    // Un rango que no se puede leer no se comprueba: se rechaza.
    if the_range_is_malformed(&headers) {
        return Err(OciFailure::blob_upload_range_mismatch(
            "the chunk declares a range that cannot be read",
        )
        .with_range(the_range_header(written)));
    }

    // El rango que declara el cliente y la longitud de su cuerpo tienen
    // que cuadrar: el protocolo dice que el `Content-Range` es el rango
    // del trozo, y un trozo que dice una cosa y mide otra es un cliente
    // que no sabe donde esta.
    if let Some((start, end)) = the_range_does_not_match_the_length(&headers) {
        return Err(OciFailure::blob_upload_range_mismatch(format!(
            "the chunk declares the range {start}-{end} and a body of another length"
        ))
        .with_range(the_range_header(written)));
    }

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

    // Un rango que no se puede leer no se comprueba: se rechaza.
    if the_range_is_malformed(&headers) {
        return Err(OciFailure::blob_upload_range_mismatch(
            "the chunk declares a range that cannot be read",
        )
        .with_range(the_range_header(already)));
    }

    // El rango que declara el cliente y la longitud de su cuerpo tienen
    // que cuadrar, por la misma razon que en el `PATCH`.
    if let Some((start, end)) = the_range_does_not_match_the_length(&headers) {
        return Err(OciFailure::blob_upload_range_mismatch(format!(
            "the chunk declares the range {start}-{end} and a body of another length"
        ))
        .with_range(the_range_header(already)));
    }

    // El rango, si el cliente lo manda. El protocolo dice que los trozos
    // van en orden: un `PUT` que cierra con un trozo que no encaja recibe
    // el mismo `416` que recibe un `PATCH` que no encaja, con el `Range`
    // que dice por donde seguir. Un `400` aqui diria "algo va mal" sin
    // decir donde esta el desfase.
    if let Some(range) = the_content_range(&headers)
        && range.0 != already
    {
        return Err(OciFailure::blob_upload_range_mismatch(format!(
            "the chunk starts at {} and the upload is at {already}",
            range.0
        ))
        .with_range(the_range_header(already)));
    }

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
    // La clase del manifiesto dice que descriptors hay que exigir, y un
    // tipo que este registro no habla no se publica.
    let Some(kind) = hodei_hub::oci::the_kind_of(&declared_type) else {
        return Err(OciFailure::manifest_invalid(format!(
            "the registry does not publish manifests of type {declared_type}"
        )));
    };

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

    // Las referencias exigidas: lo que el manifiesto nombra tiene que
    // estar en el repositorio. Un manifiesto que apunta a algo que no
    // esta es un manifiesto que no se puede descargar.
    //
    // El `subject` **no** se exige: el protocolo dice que un manifiesto
    // cuyo `subject` todavia no esta se acepta, porque un cliente puede
    // subir el referrer antes que el sujeto.
    let required = the_required_digests(&manifest, kind)?;
    let subject = the_subject_of(&manifest)?;
    let (reader, _) = the_catalog(&state)?;
    for (required, class) in &required {
        let path = match class {
            TheClass::Blob => format!("blobs/{required}"),
            TheClass::Manifest => format!("manifests/{required}"),
        };
        let asset = reader.asset(context.account().as_str(), &name, &path).await;
        if let Err(error) = asset {
            // Lo que **no** esta es un error del cliente: el manifiesto
            // nombra algo que el repositorio no tiene. Lo que el
            // catalogo no sabe es otra cosa, y se dice como lo que es.
            return Err(match error {
                hodei_catalog::CatalogError::Missing { .. } => {
                    OciFailure::manifest_blob_unknown(format!(
                        "the repository does not have the {} {required}",
                        class.as_str()
                    ))
                }
                other => crate::oci::the_catalog_error(&other),
            });
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
        // El `subject` va en la publicacion, y con ella se escribe: un
        // referrer visible a medias seria un referrer que existe cuando
        // no existe.
        subject: subject.clone(),
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
    // El `subject` del manifiesto, si lo tiene: el protocolo dice que la
    // respuesta puede decirlo, y lo dice porque el cliente que sube un
    // referrer quiere saber a que cuelga.
    if let Some(subject) = &subject
        && let Ok(value) = HeaderValue::from_str(&subject.to_string())
    {
        headers.insert(OCI_SUBJECT_HEADER, value);
    }
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(response)
}

/// Los digests que el manifiesto exige que esten en el repositorio.
///
/// La clase del manifiesto dice **que** se exige:
///
/// - Un **manifiesto de imagen** exige su configuracion y sus capas.
/// - Un **indice** exige sus hijos, que son manifiestos.
/// - Un **manifiesto de artefacto** no exige nada: su configuracion y sus
///   blobs son opcionales, y lo que nombra tiene que estar.
///
/// El digest se devuelve **sin el algoritmo**, que es como lo nombra el
/// catalogo: un blob vive en `blobs/<digest>`, y el prefijo `sha256:`
/// pertenece a la referencia OCI, no a la ruta del asset. Un digest que
/// no lo lleva no es un digest, y un manifiesto que lo nombra no se
/// puede comprobar.
fn the_required_digests(
    manifest: &serde_json::Value,
    kind: ManifestKind,
) -> Result<Vec<(String, TheClass)>, OciFailure> {
    let mut required = Vec::new();
    match kind {
        ManifestKind::Image => {
            let config = manifest
                .get("config")
                .and_then(|config| config.get("digest"))
                .and_then(|digest| digest.as_str())
                .ok_or_else(|| {
                    OciFailure::manifest_invalid("the manifest does not name a config")
                })?;
            // El descriptor **vacio** no se exige: dice "esta imagen no
            // tiene configuracion", y un registro que lo exigiera en el
            // almacen rechazaria un manifiesto que el protocolo dice que
            // es valido. Un cliente que lo manda en linea --con su
            // `data`-- nunca lo sube.
            if !the_empty_descriptor(manifest.get("config")) {
                required.push((the_hexadecimal_digest(config)?, TheClass::Blob));
            }
            if let Some(layers) = manifest.get("layers").and_then(|layers| layers.as_array()) {
                for layer in layers {
                    let Some(digest) = layer.get("digest").and_then(|digest| digest.as_str())
                    else {
                        return Err(OciFailure::manifest_invalid(
                            "a layer of the manifest does not name a digest",
                        ));
                    };
                    if the_empty_descriptor(Some(layer)) {
                        continue;
                    }
                    // Una capa **extranjera**--la que declara sus `urls`--
                    // no esta en el almacen: el cliente la baja de donde
                    // el manifiesto dice. Exigirla seria rechazar un
                    // manifiesto que el protocolo dice que es valido.
                    if the_foreign_descriptor(Some(layer)) {
                        if !the_foreign_urls_are_admisible(layer) {
                            return Err(OciFailure::manifest_invalid(
                                "the urls of a non-distributable layer must be https",
                            ));
                        }
                        continue;
                    }
                    required.push((the_hexadecimal_digest(digest)?, TheClass::Blob));
                }
            }
        }
        ManifestKind::Index => {
            let children = manifest
                .get("manifests")
                .and_then(|manifests| manifests.as_array())
                .ok_or_else(|| {
                    OciFailure::manifest_invalid("the index does not name its manifests")
                })?;
            for child in children {
                let Some(digest) = child.get("digest").and_then(|digest| digest.as_str()) else {
                    return Err(OciFailure::manifest_invalid(
                        "a manifest of the index does not name a digest",
                    ));
                };
                required.push((the_hexadecimal_digest(digest)?, TheClass::Manifest));
            }
        }
        ManifestKind::Artifact => {
            // La configuracion de un artefacto es opcional, y sus blobs
            // tambien: lo que no se nombra no se exige.
            //
            // Y lo que se nombra con el descriptor **vacio** no se
            // exige tampoco: ese descriptor dice "no hay configuracion",
            // y un registro que lo exigiera en el almacen rechazaria un
            // manifiesto que el protocolo dice que es valido.
            if let Some(config) = manifest.get("config") {
                let Some(digest) = config.get("digest").and_then(|digest| digest.as_str()) else {
                    return Err(OciFailure::manifest_invalid(
                        "the config of the artifact does not name a digest",
                    ));
                };
                if !the_empty_descriptor(Some(config)) {
                    required.push((the_hexadecimal_digest(digest)?, TheClass::Blob));
                }
            }
            if let Some(blobs) = manifest.get("blobs").and_then(|blobs| blobs.as_array()) {
                for blob in blobs {
                    let Some(digest) = blob.get("digest").and_then(|digest| digest.as_str()) else {
                        return Err(OciFailure::manifest_invalid(
                            "a blob of the artifact does not name a digest",
                        ));
                    };
                    // La capa extranjera--la no distribuible-- tampoco se
                    // exige en el almacen: el criterio es el mismo que en
                    // las capas de una imagen, y una exencion que depende
                    // de la forma del manifiesto es una exencion que se
                    // olvida.
                    if the_foreign_descriptor(Some(blob)) {
                        if !the_foreign_urls_are_admisible(blob) {
                            return Err(OciFailure::manifest_invalid(
                                "the urls of a non-distributable blob must be https",
                            ));
                        }
                        continue;
                    }
                    required.push((the_hexadecimal_digest(digest)?, TheClass::Blob));
                }
            }
        }
    }
    Ok(required)
}

/// Si un descriptor es el descriptor **vacio**.
///
/// El descriptor vacio dice "esto no hay": un manifiesto de artefacto
/// sin configuracion, o una imagen sin configuracion. Los bytes que
/// nombra son un `{}` de dos bytes, y un cliente puede mandarlos en
/// linea --con su `data`-- sin subirlos nunca al almacen. Un registro
/// que los exigiera en el almacen rechazaria un manifiesto valido.
fn the_empty_descriptor(descriptor: Option<&serde_json::Value>) -> bool {
    let Some(descriptor) = descriptor else {
        return false;
    };
    let media_type = descriptor
        .get("mediaType")
        .and_then(|media_type| media_type.as_str())
        .unwrap_or_default();
    let digest = descriptor
        .get("digest")
        .and_then(|digest| digest.as_str())
        .unwrap_or_default();
    hodei_hub::oci::is_the_empty_descriptor(media_type, digest)
}

/// Si un descriptor es el de una capa **extranjera**.
///
/// La capa extranjera es la **no distribuible**: su tipo de medio lo dice
/// (`application/vnd.oci.image.layer.nondistributable.v1.tar`, con o sin
/// `+gzip`), y sus `urls` dicen de donde se baja. El registro no la tiene
/// ni la tiene que tener, y un manifiesto que la nombra es un manifiesto
/// valido: exigir la capa seria rechazar un manifiesto que el protocolo
/// dice que es valido.
///
/// La exencion es **exactamente** la de la capa no distribuible, y no la
/// de cualquier descriptor con `urls`: un descriptor que declara `urls` y
/// un tipo de medio cualquiera es una capa normal con una pista, y
/// eximirla seria dejar de exigir en el almacen un contenido que el
/// manifiesto nombra. El tipo de medio es lo que la distingue.
fn the_foreign_descriptor(descriptor: Option<&serde_json::Value>) -> bool {
    let Some(descriptor) = descriptor else {
        return false;
    };
    let Some(media_type) = descriptor.get("mediaType").and_then(|media| media.as_str()) else {
        return false;
    };
    if !THE_FOREIGN_MEDIA_TYPES.contains(&media_type) {
        return false;
    }
    descriptor
        .get("urls")
        .and_then(|urls| urls.as_array())
        .is_some_and(|urls| !urls.is_empty())
}

/// Si las `urls` de una capa no distribuible son admisibles.
///
/// La capa se baja de donde el manifiesto dice, y ese "donde" lo decide
/// quien publica: un `http` a una direccion interna--la de las
/// credenciales de la nube-- es un cliente que va a pedir las
/// credenciales de la maquina que tira de la imagen. Por eso solo se
/// admiten `https`: el protocolo permite cualquier URI, y una plataforma
/// de cadena de suministro no puede ir publicando direcciones que el
/// cliente va a seguir.
///
/// La exencion de la capa es necesaria--el protocolo la tiene--, pero la
/// exencion no tiene por que ser una puerta abierta.
fn the_foreign_urls_are_admisible(descriptor: &serde_json::Value) -> bool {
    let Some(urls) = descriptor.get("urls").and_then(|urls| urls.as_array()) else {
        return false;
    };
    urls.iter()
        .all(|url| url.as_str().is_some_and(|url| url.starts_with("https://")))
}

/// Los tipos de medio de una capa **no distribuible**.
///
/// Son los que define la especificacion de imagen, y son los unicos que
/// se eximen de estar en el almacen.
const THE_FOREIGN_MEDIA_TYPES: [&str; 2] = [
    "application/vnd.oci.image.layer.nondistributable.v1.tar",
    "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip",
];

/// La clase de lo que un manifiesto nombra.
///
/// Un blob vive en `blobs/<digest>` y un manifiesto en
/// `manifests/<digest>`: son rutas distintas, y un manifiesto que exige
/// uno no puede servir el otro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TheClass {
    /// Un blob del repositorio.
    Blob,
    /// Un manifiesto del repositorio.
    Manifest,
}

impl TheClass {
    /// El nombre de la clase, para el mensaje del error.
    fn as_str(&self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::Manifest => "manifest",
        }
    }
}

/// El `subject` del manifiesto, si lo tiene.
///
/// Es el digest del manifiesto al que este cuelga, y **no** se exige que
/// exista: el protocolo dice que un registro tiene que aceptar un
/// manifiesto cuyo `subject` todavia no esta, porque un cliente puede
/// subir el referrer antes que el sujeto.
fn the_subject_of(manifest: &serde_json::Value) -> Result<Option<ContentDigest>, OciFailure> {
    let Some(subject) = manifest.get("subject") else {
        return Ok(None);
    };
    let Some(digest) = subject.get("digest").and_then(|digest| digest.as_str()) else {
        return Err(OciFailure::manifest_invalid(
            "the subject does not name a digest",
        ));
    };
    Ok(Some(ContentDigest::sha256(the_hexadecimal_digest(digest)?)))
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
    // Y tiene que ser **un** digest: un hexadecimal a medio escribir no
    // nombra un contenido, y un manifiesto que lo nombra no se puede
    // comprobar. Sin esta comprobacion, un `subject` a medio escribir
    // se guardaria en el catalogo y el manifiesto que lo declaraba
    // quedaria publicado y ilegible para siempre.
    let candidate = ContentDigest::sha256(hexadecimal);
    if !candidate.is_well_formed() {
        return Err(OciFailure::manifest_invalid(format!(
            "the digest {digest:?} is not a well formed digest"
        )));
    }
    Ok(candidate.value)
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
    // HTTP escribe el rango con unidad y con total (`bytes 0-99/200`), y
    // el protocolo de OCI lo escribe desnudo (`0-99`). Las dos se leen.
    let value = value.strip_prefix("bytes ").unwrap_or(value).trim();
    // El total no dice donde va el trozo: se ignora.
    let value = value.split('/').next().unwrap_or(value).trim();
    let (start, end) = value.split_once('-')?;
    Some((start.trim().parse().ok()?, end.trim().parse().ok()?))
}

/// Si el rango que declara el cliente esta **mal formado**.
///
/// El protocolo dice que el rango tiene que estar bien formado, y un
/// cliente que lo manda mal no puede escribir donde quiere: un rango que
/// no se entiende es un rango que no se puede satisfacer. Sin esta
/// comprobacion, un `Content-Range` que no se sabe leer se pasa por alto y
/// el trozo se acepta sin comprobar nada.
fn the_range_is_malformed(headers: &HeaderMap) -> bool {
    headers.get("content-range").is_some() && the_content_range(headers).is_none()
}

/// Si el rango que declara el cliente no cuadra con la longitud de su
/// cuerpo.
///
/// El protocolo dice que el `Content-Range` es el rango del trozo y que el
/// `Content-Length` es la longitud del trozo: los dos dicen lo mismo. Un
/// cliente que declara una cosa y manda otra no sabe donde esta, y la
/// respuesta es la misma que recibe el trozo fuera de orden: un `416` con
/// el `Range` que dice por donde seguir.
///
/// Sin `Content-Range` no hay nada que comparar: el rango es opcional.
fn the_range_does_not_match_the_length(headers: &HeaderMap) -> Option<(u64, u64)> {
    let (start, end) = the_content_range(headers)?;
    let length: u64 = headers
        .get(axum::http::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    if end < start || end - start + 1 != length {
        return Some((start, end));
    }
    None
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
        subject: None,
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

    /// Un digest de verdad: los tests no tienen que medir la longitud
    /// del digest, tienen que medir **que** digest se exige.
    const A_DIGEST: &str =
        "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const B_DIGEST: &str =
        "sha256:2222222222222222222222222222222222222222222222222222222222222222";
    const C_DIGEST: &str =
        "sha256:3333333333333333333333333333333333333333333333333333333333333333";

    /// El hexadecimal de un digest de verdad.
    fn the_hexadecimal(digest: &str) -> &str {
        digest
            .strip_prefix("sha256:")
            .expect("the digest carries its algorithm")
    }

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

    /// El rango declarado y la longitud del cuerpo tienen que cuadrar.
    ///
    /// El protocolo dice que el `Content-Range` es el rango del trozo y
    /// que el `Content-Length` es su longitud: los dos dicen lo mismo.
    #[test]
    fn a_range_that_does_not_match_the_length_is_a_mismatch() {
        let mut headers = HeaderMap::new();
        headers.insert("content-range", "0-99".parse().expect("a header"));
        headers.insert("content-length", "100".parse().expect("a header"));
        assert_eq!(the_range_does_not_match_the_length(&headers), None);

        // El mismo rango con un cuerpo de otra longitud.
        headers.insert("content-length", "3".parse().expect("a header"));
        assert_eq!(the_range_does_not_match_the_length(&headers), Some((0, 99)));

        // Un rango al reves: el final es anterior al inicio.
        headers.insert("content-range", "99-0".parse().expect("a header"));
        headers.insert("content-length", "100".parse().expect("a header"));
        assert_eq!(the_range_does_not_match_the_length(&headers), Some((99, 0)));
    }

    /// Sin `Content-Range` no hay nada que comparar: el rango es
    /// opcional, y el `Content-Length` solo no dice nada.
    #[test]
    fn without_a_range_there_is_nothing_to_compare() {
        let mut headers = HeaderMap::new();
        headers.insert("content-length", "100".parse().expect("a header"));
        assert_eq!(the_range_does_not_match_the_length(&headers), None);
    }

    /// La capa **no distribuible** es la que se exime de estar en el
    /// almacen: su tipo de medio lo dice, y sus `urls` dicen de donde se
    /// baja.
    #[test]
    fn a_foreign_layer_is_the_one_that_is_not_distributed() {
        let layer = serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip",
            "digest": A_DIGEST,
            "urls": ["https://example.invalid/layer"],
        });
        assert!(the_foreign_descriptor(Some(&layer)));

        let layer = serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "digest": A_DIGEST,
            "urls": ["https://example.invalid/layer"],
        });
        assert!(!the_foreign_descriptor(Some(&layer)));
    }

    /// Una capa normal sin `urls` no es extranjera: se exige en el
    /// almacen, como cualquier otra.
    #[test]
    fn a_normal_layer_is_not_foreign() {
        let layer = serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "digest": A_DIGEST,
        });
        assert!(!the_foreign_descriptor(Some(&layer)));
    }

    /// El rango se lee en las dos grafias: la del protocolo (`0-99`) y la
    /// de HTTP (`bytes 0-99/200`).
    #[test]
    fn the_content_range_is_read_in_both_spellings() {
        let mut headers = HeaderMap::new();
        headers.insert("content-range", "bytes 0-99/200".parse().expect("a header"));
        assert_eq!(the_content_range(&headers), Some((0, 99)));

        headers.insert("content-range", "bytes 100-199".parse().expect("a header"));
        assert_eq!(the_content_range(&headers), Some((100, 199)));
    }

    /// Un rango que esta y no se puede leer es un rango mal formado: no
    /// se pasa por alto.
    #[test]
    fn a_range_that_cannot_be_read_is_malformed() {
        let mut headers = HeaderMap::new();
        headers.insert("content-range", "not-a-range".parse().expect("a header"));
        assert!(the_range_is_malformed(&headers));

        headers.insert("content-range", "0-99".parse().expect("a header"));
        assert!(!the_range_is_malformed(&headers));

        // Sin cabecera no hay nada mal formado: el rango es opcional.
        assert!(!the_range_is_malformed(&HeaderMap::new()));
    }

    /// Las `urls` de la capa no distribuible tienen que ser `https`: el
    /// cliente va a seguirlas, y quien publica decide a donde va.
    #[test]
    fn the_urls_of_a_foreign_layer_have_to_be_https() {
        let layer = serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.nondistributable.v1.tar",
            "digest": A_DIGEST,
            "urls": ["https://store.example.com/blobs/sha256/x"],
        });
        assert!(the_foreign_urls_are_admisible(&layer));

        let layer = serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.nondistributable.v1.tar",
            "digest": A_DIGEST,
            "urls": ["http://169.254.169.254/latest/meta-data/"],
        });
        assert!(!the_foreign_urls_are_admisible(&layer));

        let layer = serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.nondistributable.v1.tar",
            "digest": A_DIGEST,
            "urls": ["file:///etc/passwd"],
        });
        assert!(!the_foreign_urls_are_admisible(&layer));
    }

    /// El rango que dice la subida es `0-<ultimo byte>`.
    #[test]
    fn the_range_of_the_upload_is_the_last_byte() {
        assert_eq!(the_range_header(0), "0-0");
        assert_eq!(the_range_header(1), "0-0");
        assert_eq!(the_range_header(100), "0-99");
    }

    /// Las referencias exigidas de un manifiesto de imagen son su config
    /// y sus capas, y nada mas.
    ///
    /// El digest se queda **sin el algoritmo**: es como lo nombra el
    /// catalogo, que guarda el blob en `blobs/<digest>`.
    #[test]
    fn the_required_digests_are_the_config_and_the_layers() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": { "digest": A_DIGEST },
            "layers": [{ "digest": B_DIGEST }, { "digest": C_DIGEST }],
        });
        let required = the_required_digests(&manifest, ManifestKind::Image).expect("the digests");
        assert_eq!(
            required,
            vec![
                (the_hexadecimal(A_DIGEST).to_string(), TheClass::Blob),
                (the_hexadecimal(B_DIGEST).to_string(), TheClass::Blob),
                (the_hexadecimal(C_DIGEST).to_string(), TheClass::Blob),
            ]
        );
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
        let error = the_required_digests(&manifest, ManifestKind::Image)
            .expect_err("there is no algorithm");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");

        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": { "digest": "md5:aaa" },
            "layers": [],
        });
        let error = the_required_digests(&manifest, ManifestKind::Image)
            .expect_err("the algorithm is not sha256");
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
        let error =
            the_required_digests(&manifest, ManifestKind::Image).expect_err("there is no config");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");
    }

    /// Un indice exige sus hijos, que son **manifiestos** y no blobs.
    #[test]
    fn an_index_requires_its_children_as_manifests() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": [
                { "digest": A_DIGEST, "platform": { "os": "linux", "architecture": "amd64" } },
                { "digest": B_DIGEST, "platform": { "os": "linux", "architecture": "arm64" } }
            ],
        });
        let required = the_required_digests(&manifest, ManifestKind::Index).expect("the digests");
        assert_eq!(
            required,
            vec![
                (the_hexadecimal(A_DIGEST).to_string(), TheClass::Manifest),
                (the_hexadecimal(B_DIGEST).to_string(), TheClass::Manifest),
            ]
        );

        // Un indice sin hijos no es un indice.
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
        });
        let error = the_required_digests(&manifest, ManifestKind::Index)
            .expect_err("there are no children");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");
    }

    /// Un manifiesto de artefacto no exige nada, y lo que nombra son
    /// blobs.
    #[test]
    fn an_artifact_manifest_requires_nothing_and_names_blobs() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
            "artifactType": "application/vnd.example.sbom.v1",
            "blobs": [{ "digest": A_DIGEST }],
        });
        let required =
            the_required_digests(&manifest, ManifestKind::Artifact).expect("the digests");
        assert_eq!(
            required,
            vec![(the_hexadecimal(A_DIGEST).to_string(), TheClass::Blob)]
        );

        // Sin blobs ni configuracion, un artefacto no exige nada.
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
            "artifactType": "application/vnd.example.sbom.v1",
        });
        let required =
            the_required_digests(&manifest, ManifestKind::Artifact).expect("the digests");
        assert!(required.is_empty());
    }

    /// El `subject` se lee, y **no** se exige que exista: el protocolo
    /// dice que un manifiesto cuyo `subject` todavia no esta se acepta.
    #[test]
    fn the_subject_is_read_and_not_required_to_exist() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
            "artifactType": "application/vnd.example.sbom.v1",
            "subject": { "digest": A_DIGEST },
        });
        let subject = the_subject_of(&manifest).expect("the subject");
        assert_eq!(
            subject.map(|digest| digest.value),
            Some(the_hexadecimal(A_DIGEST).to_string())
        );

        // Sin subject, no hay subject.
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
        });
        assert!(the_subject_of(&manifest).expect("no subject").is_none());

        // Y un subject que **no** es un digest no es un subject: un
        // digest a medio escribir se guardaria en el catalogo y dejaria
        // el manifiesto publicado y ilegible.
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
            "subject": { "digest": "sha256:aaa" },
        });
        let error = the_subject_of(&manifest).expect_err("the subject is not a digest");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");

        // Un subject sin digest no es un subject.
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
            "subject": { "size": 12 },
        });
        let error = the_subject_of(&manifest).expect_err("there is no digest");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");
    }
}
