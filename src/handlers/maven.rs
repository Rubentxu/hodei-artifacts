//! Maven 2 hosted sobre la superficie de contenido.
//!
//! Un repositorio Maven 2 hosted usa las mismas rutas y el mismo modelo que
//! el crudo (`/repository/{repo}/{*path}`): cada fichero es un asset cuyo
//! contenido vive en el almacen por digest. Lo que cambia es lo que se
//! decide **antes** de guardar y **al** servir lo que Maven llama
//! metadatos:
//!
//! - **Escribir.** La ruta tiene que ser una ruta Maven 2 valida, la version
//!   tiene que caber en la politica del repositorio, y la sustitucion no la
//!   decide el cliente: una release no cambia (salvo `release_redeploy`), un
//!   snapshot con marca de tiempo no cambia, y el nombre flotante
//!   `-SNAPSHOT` se repone. Un POM se lee antes de guardarlo y sus
//!   coordenadas tienen que ser las de su ruta.
//! - **`maven-metadata.xml` y checksums.** No se guardan: se **derivan** del
//!   catalogo cada vez que se piden. Asi no hay un fichero de metadatos que
//!   pueda quedar a medias ni dos despliegues concurrentes que se lo pisen:
//!   lo que se lista es lo que hay publicado, y una version aparece cuando
//!   existe su POM. Lo que un cliente sube con esos nombres se **valida** y
//!   se acepta sin guardar, para que `mvn deploy` termine igual que contra
//!   cualquier otro repositorio.
//! - **Checksums de un fichero.** Se comparan con el resumen calculado del
//!   contenido guardado: si el cliente sube uno que no es el del fichero, se
//!   rechaza; si lo es, se acepta (el servidor ya lo sabe) sin guardarlo.

use std::collections::HashSet;

use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use hodei_authz::VerifiedContext;
use hodei_catalog::maven::{
    self, Algorithm, Known, MavenConfig, MavenFile, MetadataLevel, Subject, Target, VersionKind,
};
use hodei_catalog::{AssetRecord, CatalogError, RepositoryDescriptor, RepositoryMode};
use hodei_evidence::domain::content::ContentDigest;
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

use super::raw::{
    RawApiError, apply_common_headers, hex_of, the_admission_error, the_blob_error, the_catalog,
    the_content_store, the_current_asset,
};
use crate::app_state::AppState;
use crate::raw::{conditional, policy::ReplacePolicy};

/// Cuanto se lee del almacen de una vez al calcular un resumen: un jar de un
/// gigabyte no cabe en memoria.
const HASH_CHUNK: u64 = 4 * 1024 * 1024;

/// Lo que una escritura pide hacer.
pub(super) enum Write {
    /// Un fichero del artefacto: se guarda.
    File {
        /// El fichero, ya analizado.
        file: MavenFile,
        /// Si su direccion se puede reponer.
        replace: ReplacePolicy,
    },
    /// Metadatos o checksum: se validan y no se guardan.
    Absorbed(Absorbed),
}

/// Lo que se valida y se acepta sin guardar.
pub(super) enum Absorbed {
    /// `maven-metadata.xml`.
    Metadata(MetadataLevel),
    /// Un checksum de `maven-metadata.xml`.
    MetadataChecksum(Algorithm),
    /// Un checksum de un fichero del artefacto.
    FileChecksum {
        /// El algoritmo.
        algorithm: Algorithm,
        /// La ruta del fichero del que es checksum.
        subject_path: String,
    },
}

/// La configuracion Maven de un repositorio.
fn the_config(repository: &RepositoryDescriptor) -> Result<MavenConfig, RawApiError> {
    // Se valida al guardar el repositorio; si ahora no se entiende, el
    // repositorio esta mal y no se sirve a medias.
    MavenConfig::of_settings(&repository.config.config_settings).map_err(|error| {
        tracing::warn!(error = %error, "the maven settings of a repository cannot be read");
        RawApiError::Unavailable
    })
}

/// Que hay que hacer con una escritura en una ruta de un repositorio Maven 2.
pub(super) fn the_write(
    repository: &RepositoryDescriptor,
    asset_path: &str,
) -> Result<Write, RawApiError> {
    let config = the_config(repository)?;
    let target =
        maven::parse_path(asset_path).map_err(|error| RawApiError::BadRequest(error.message()))?;
    match target {
        Target::File(file) => {
            if !config.admits(file.is_snapshot()) {
                return Err(RawApiError::BadRequest(
                    "this repository does not admit this kind of version",
                ));
            }
            let replace = match file.kind {
                VersionKind::Release if config.release_redeploy => ReplacePolicy::Allow,
                VersionKind::Release | VersionKind::Timestamped { .. } => ReplacePolicy::Deny,
                // El nombre flotante es el que un snapshot repone.
                VersionKind::Snapshot => ReplacePolicy::Allow,
            };
            Ok(Write::File { file, replace })
        }
        Target::Metadata(level) => {
            if let MetadataLevel::Version { version, .. } = &level
                && !config.admits(version.ends_with(maven::SNAPSHOT_SUFFIX))
            {
                return Err(RawApiError::BadRequest(
                    "this repository does not admit this kind of version",
                ));
            }
            Ok(Write::Absorbed(Absorbed::Metadata(level)))
        }
        Target::Checksum {
            algorithm,
            subject,
            subject_path,
        } => match subject {
            Subject::Metadata(_) => Ok(Write::Absorbed(Absorbed::MetadataChecksum(algorithm))),
            Subject::File(file) => {
                if !config.admits(file.is_snapshot()) {
                    return Err(RawApiError::BadRequest(
                        "this repository does not admit this kind of version",
                    ));
                }
                Ok(Write::Absorbed(Absorbed::FileChecksum {
                    algorithm,
                    subject_path,
                }))
            }
        },
    }
}

/// Lee un cuerpo entero con un tope.
async fn read_limited(body: Body, limit: usize) -> Result<Vec<u8>, RawApiError> {
    let mut stream = body.into_data_stream();
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| RawApiError::BadRequest("the body could not be read"))?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(RawApiError::PayloadTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// El cuerpo de un fichero, listo para guardar.
///
/// Un POM se lee entero antes de guardarlo: sus coordenadas tienen que ser
/// las de su ruta, y un POM que no lo es no deja ni un byte en el almacen.
/// El resto de ficheros pasa por trozos, sin tocarse.
pub(super) async fn the_file_body(file: &MavenFile, body: Body) -> Result<Body, RawApiError> {
    if !file.is_pom() {
        return Ok(body);
    }
    let bytes = read_limited(body, maven::MAX_XML_BYTES).await?;
    let declared =
        maven::parse_pom(&bytes).map_err(|error| RawApiError::BadRequest(error.message()))?;
    if declared.group_id != file.group_id
        || declared.artifact_id != file.artifact_id
        || declared.version != file.version
    {
        return Err(RawApiError::BadRequest(
            "the POM coordinates do not match the path",
        ));
    }
    Ok(Body::from(bytes))
}

/// El resumen de un contenido guardado, leido por trozos.
async fn checksum_of(
    state: &AppState,
    asset: &AssetRecord,
    algorithm: Algorithm,
) -> Result<String, RawApiError> {
    if algorithm == Algorithm::Sha256 {
        // El digest del contenido ya es su SHA-256.
        return Ok(asset.digest.value.to_ascii_lowercase());
    }
    let store = the_content_store(state)?;
    let mut hasher = Hasher::of(algorithm);
    let mut offset: u64 = 0;
    while offset < asset.size_bytes {
        let want = HASH_CHUNK.min(asset.size_bytes - offset);
        let chunk = store
            .read_range(&asset.digest, offset, want)
            .await
            .map_err(|error| the_blob_error(&error))?;
        if chunk.is_empty() {
            // Un almacen que devuelve menos de lo que dice el catalogo esta
            // roto: no se inventa un resumen.
            return Err(RawApiError::Internal);
        }
        hasher.update(&chunk);
        offset += chunk.len() as u64;
    }
    Ok(hasher.finish())
}

/// Un resumen en curso.
enum Hasher {
    Md5(Md5),
    Sha1(Sha1),
    Sha256(Sha256),
    Sha512(Sha512),
}

impl Hasher {
    fn of(algorithm: Algorithm) -> Self {
        match algorithm {
            Algorithm::Md5 => Self::Md5(Md5::new()),
            Algorithm::Sha1 => Self::Sha1(Sha1::new()),
            Algorithm::Sha256 => Self::Sha256(Sha256::new()),
            Algorithm::Sha512 => Self::Sha512(Sha512::new()),
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Md5(hasher) => hasher.update(bytes),
            Self::Sha1(hasher) => hasher.update(bytes),
            Self::Sha256(hasher) => hasher.update(bytes),
            Self::Sha512(hasher) => hasher.update(bytes),
        }
    }

    fn finish(self) -> String {
        match self {
            Self::Md5(hasher) => hex_of(&hasher.finalize()),
            Self::Sha1(hasher) => hex_of(&hasher.finalize()),
            Self::Sha256(hasher) => hex_of(&hasher.finalize()),
            Self::Sha512(hasher) => hex_of(&hasher.finalize()),
        }
    }
}

/// El resumen de unos bytes en memoria.
fn checksum_of_bytes(bytes: &[u8], algorithm: Algorithm) -> String {
    let mut hasher = Hasher::of(algorithm);
    hasher.update(bytes);
    hasher.finish()
}

/// Valida lo que un cliente sube con nombre de metadatos o de checksum, y lo
/// acepta sin guardarlo.
pub(super) async fn absorb(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    absorbed: Absorbed,
    body: Body,
    correlation: &str,
) -> Result<Response, RawApiError> {
    let reason = match absorbed {
        Absorbed::Metadata(level) => {
            let bytes = read_limited(body, maven::MAX_XML_BYTES).await?;
            let declared = maven::parse_metadata(&bytes)
                .map_err(|_| RawApiError::BadRequest("the metadata is not a valid XML document"))?;
            if !maven::metadata_matches(&level, &declared) {
                return Err(RawApiError::BadRequest(
                    "the metadata does not match its path",
                ));
            }
            "metadata"
        }
        Absorbed::MetadataChecksum(algorithm) => {
            let bytes = read_limited(body, maven::MAX_CHECKSUM_BYTES).await?;
            maven::declared_checksum(&bytes, algorithm).ok_or(RawApiError::BadRequest(
                "the checksum is not valid for its algorithm",
            ))?;
            "metadata-checksum"
        }
        Absorbed::FileChecksum {
            algorithm,
            subject_path,
        } => {
            let bytes = read_limited(body, maven::MAX_CHECKSUM_BYTES).await?;
            let declared = maven::declared_checksum(&bytes, algorithm).ok_or(
                RawApiError::BadRequest("the checksum is not valid for its algorithm"),
            )?;
            let Some(asset) =
                the_current_asset(state, context, &repository.repo_key, &subject_path).await?
            else {
                return Err(RawApiError::Conflict(
                    "the file of this checksum has not been published",
                ));
            };
            let found = checksum_of(state, &asset, algorithm).await?;
            if found != declared {
                return Err(RawApiError::BadRequest(
                    "the checksum does not match the content",
                ));
            }
            "checksum"
        }
    };
    let payload = serde_json::json!({
        "repository": repository.repo_key,
        "path": asset_path,
        "accepted": true,
        "stored": false,
        "kind": reason,
    });
    let mut response = (StatusCode::OK, axum::Json(payload)).into_response();
    if let Ok(value) = HeaderValue::from_str(correlation) {
        response
            .headers_mut()
            .insert(header::HeaderName::from_static("x-request-id"), value);
    }
    Ok(response)
}

/// Lo que hay publicado bajo un prefijo, con los digests que la compuerta de
/// confianza deja servir.
async fn known_under(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    prefix: &str,
) -> Result<Vec<Known>, RawApiError> {
    let (reader, _) = the_catalog(state)?;
    let assets = reader
        .assets_under(context.account().as_str(), &repository.repo_key, prefix)
        .await
        .map_err(|error| match error {
            CatalogError::Denied { .. } => RawApiError::Forbidden,
            _ => RawApiError::Unavailable,
        })?;
    let servable: Option<HashSet<String>> =
        crate::trust::servable(state, context.account().as_str(), repository)
            .await
            .map_err(the_admission_error)?;
    Ok(assets
        .into_iter()
        .filter(|asset| {
            servable
                .as_ref()
                .is_none_or(|set| set.contains(&asset.digest.value))
        })
        .map(|asset| Known {
            path: asset.asset_path,
            updated_at: asset.updated_at,
        })
        .collect())
}

/// Los metadatos derivados de lo publicado, si hay algo que describir.
async fn derived_metadata(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    level: &MetadataLevel,
) -> Result<Option<String>, RawApiError> {
    let prefix = match level {
        MetadataLevel::Artifact {
            group_id,
            artifact_id,
        } => format!("{}/{artifact_id}/", group_id.replace('.', "/")),
        MetadataLevel::Version {
            group_id,
            artifact_id,
            version,
        } => format!("{}/{artifact_id}/{version}/", group_id.replace('.', "/")),
    };
    let known = known_under(state, context, repository, &prefix).await?;
    Ok(match level {
        MetadataLevel::Artifact {
            group_id,
            artifact_id,
        } => maven::artifact_metadata(group_id, artifact_id, &known),
        MetadataLevel::Version {
            group_id,
            artifact_id,
            version,
        } => maven::snapshot_metadata(group_id, artifact_id, version, &known),
    })
}

/// Lo que se sirve en una ruta de metadatos o de checksum.
///
/// `Ok(None)` dice que la ruta no es de metadatos ni de checksum: la sirve el
/// flujo normal del fichero.
pub(super) async fn the_derived(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    asset_path: &str,
    headers: &HeaderMap,
    head: bool,
    correlation: &str,
) -> Result<Option<Response>, RawApiError> {
    let Ok(target) = maven::parse_path(asset_path) else {
        return Ok(None);
    };
    let (content, content_type): (Vec<u8>, &'static str) = match target {
        Target::File(_) => return Ok(None),
        Target::Metadata(level) => {
            let xml = derived_metadata(state, context, repository, &level)
                .await?
                .ok_or(RawApiError::NotFound)?;
            (xml.into_bytes(), "application/xml")
        }
        Target::Checksum {
            algorithm,
            subject,
            subject_path,
        } => {
            let text = match subject {
                Subject::Metadata(level) => {
                    let xml = derived_metadata(state, context, repository, &level)
                        .await?
                        .ok_or(RawApiError::NotFound)?;
                    checksum_of_bytes(xml.as_bytes(), algorithm)
                }
                Subject::File(_) => {
                    let asset =
                        the_current_asset(state, context, &repository.repo_key, &subject_path)
                            .await?
                            .ok_or(RawApiError::NotFound)?;
                    // El resumen de un fichero es del fichero: la compuerta
                    // de confianza decide sobre su digest, igual que al
                    // descargarlo.
                    crate::trust::admit(
                        state,
                        context.account().as_str(),
                        repository,
                        &asset.digest,
                    )
                    .await
                    .map_err(the_admission_error)?;
                    checksum_of(state, &asset, algorithm).await?
                }
            };
            (text.into_bytes(), "text/plain; charset=utf-8")
        }
    };

    respond_derived(
        content,
        content_type,
        headers,
        head,
        correlation,
        &GroupTrace::default(),
    )
    .map(Some)
}

/// Lo que un grupo anade a una respuesta derivada: quien aporto el contenido
/// y en que version de su configuracion.
#[derive(Default)]
pub(super) struct GroupTrace {
    /// Los miembros que aportaron, separados por comas.
    pub from: Option<String>,
    /// La version de la configuracion del grupo.
    pub version: Option<u64>,
}

fn apply_trace(response: &mut Response, trace: &GroupTrace) {
    if let Some(from) = &trace.from
        && let Ok(value) = HeaderValue::from_str(from)
    {
        response.headers_mut().insert(
            header::HeaderName::from_static("x-hodei-resolved-from"),
            value,
        );
    }
    if let Some(version) = trace.version {
        response.headers_mut().insert(
            header::HeaderName::from_static("x-hodei-group-version"),
            HeaderValue::from(version),
        );
    }
}

/// La respuesta de un contenido derivado: condicionales, cabeceras y cuerpo.
fn respond_derived(
    content: Vec<u8>,
    content_type: &'static str,
    headers: &HeaderMap,
    head: bool,
    correlation: &str,
    trace: &GroupTrace,
) -> Result<Response, RawApiError> {
    let etag = conditional::the_etag(&ContentDigest::of_bytes(&content));
    match conditional::the_read_condition(
        conditional::the_if_match(headers).as_deref(),
        conditional::the_if_none_match(headers).as_deref(),
        Some(&etag),
    ) {
        conditional::ReadOutcome::Failed => return Err(RawApiError::PreconditionFailed),
        conditional::ReadOutcome::NotModified => {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            apply_common_headers(&mut response, &etag, correlation);
            apply_trace(&mut response, trace);
            return Ok(response);
        }
        conditional::ReadOutcome::Serve => {}
    }

    let length = content.len();
    let body = if head {
        Body::empty()
    } else {
        Body::from(content)
    };
    let mut response = (StatusCode::OK, body).into_response();
    apply_common_headers(&mut response, &etag, correlation);
    apply_trace(&mut response, trace);
    let response_headers = response.headers_mut();
    response_headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response_headers.insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    // Derivado de lo publicado: quien lo guarde tiene que revalidarlo.
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response_headers.insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// Grupos y proxies.
// ---------------------------------------------------------------------------

/// Si la politica de versiones de un repositorio Maven 2 admite la ruta.
///
/// Un proxy aplica la misma politica que un hosted (`release` / `snapshot`):
/// una version que el repositorio no admite no se pide al upstream. Lo que
/// no es un fichero ni unos metadatos de version se deja pasar: el proxy no
/// inventa reglas sobre rutas que no entiende.
pub(crate) fn admits_path(
    repository: &RepositoryDescriptor,
    asset_path: &str,
) -> Result<bool, RawApiError> {
    let config = the_config(repository)?;
    let snapshot = match maven::parse_path(asset_path) {
        Ok(Target::File(file)) => file.is_snapshot(),
        Ok(Target::Metadata(MetadataLevel::Version { version, .. })) => {
            version.ends_with(maven::SNAPSHOT_SUFFIX)
        }
        Ok(Target::Checksum {
            subject: Subject::File(file),
            ..
        }) => file.is_snapshot(),
        Ok(Target::Checksum {
            subject: Subject::Metadata(MetadataLevel::Version { version, .. }),
            ..
        }) => version.ends_with(maven::SNAPSHOT_SUFFIX),
        // Lo que no se entiende pasa, salvo que cuelgue de una carpeta
        // `-SNAPSHOT`: ahi un nombre raro no es una forma de esquivar la
        // politica.
        _ => {
            let in_snapshot_directory = asset_path
                .rsplit('/')
                .skip(1)
                .any(|segment| segment.ends_with(maven::SNAPSHOT_SUFFIX));
            return Ok(!in_snapshot_directory || config.admits(true));
        }
    };
    Ok(config.admits(snapshot))
}

/// Lo que un miembro hoja de un grupo aporta a unos metadatos.
struct Piece {
    /// Los bytes que el miembro sirve en esa ruta de metadatos.
    bytes: Vec<u8>,
    /// Lo que aportan a la lista de versiones del artefacto.
    contribution: maven::Contribution,
    /// Si lo sirve un upstream tal cual (y no se deriva de lo publicado).
    verbatim: bool,
}

/// Lee un fichero de metadatos que un proxy tiene, con tope de tamano.
async fn read_cached(state: &AppState, asset: &AssetRecord) -> Result<Vec<u8>, RawApiError> {
    if asset.size_bytes > maven::MAX_XML_BYTES as u64 {
        tracing::warn!("a proxied maven metadata file is larger than the limit");
        return Err(RawApiError::BadGateway(
            "the upstream metadata is larger than the limit",
        ));
    }
    let store = the_content_store(state)?;
    let bytes = store
        .get(&asset.digest)
        .await
        .map_err(|error| the_blob_error(&error))?;
    Ok(bytes.to_vec())
}

/// Lo que un miembro hoja aporta a los metadatos de `level`.
///
/// `Ok(None)` es un MISS: el miembro no tiene metadatos para esa ruta, o la
/// compuerta de confianza oculta el fichero que los respalda.
async fn the_piece(
    state: &AppState,
    context: &VerifiedContext,
    member: &RepositoryDescriptor,
    level: &MetadataLevel,
    metadata_path: &str,
    correlation: &str,
) -> Result<Option<Piece>, RawApiError> {
    super::raw_group::global_only(member)?;
    if member.mode == RepositoryMode::Proxy {
        if !admits_path(member, metadata_path)? {
            return Ok(None);
        }
        let served =
            match super::raw::the_proxy_asset(state, context, member, metadata_path, correlation)
                .await
            {
                Ok(served) => served,
                Err(RawApiError::NotFound) => return Ok(None),
                Err(other) => return Err(other),
            };
        match crate::trust::admit(
            state,
            context.account().as_str(),
            member,
            &served.asset.digest,
        )
        .await
        {
            Ok(()) => {}
            // Los de artefacto son una union: un miembro que los oculta no
            // aporta. Los de version los sirve un solo miembro: ocultos es
            // un 404 y no se busca en el siguiente, como con un fichero.
            Err(crate::trust::Admission::Hidden) => {
                return match level {
                    MetadataLevel::Artifact { .. } => Ok(None),
                    MetadataLevel::Version { .. } => Err(RawApiError::NotFound),
                };
            }
            Err(other) => return Err(the_admission_error(other)),
        }
        let bytes = read_cached(state, &served.asset).await?;
        let contribution = match level {
            MetadataLevel::Artifact {
                group_id,
                artifact_id,
            } => {
                let mut declared = maven::declared_contribution(&bytes, group_id, artifact_id)
                    .map_err(|_| RawApiError::BadGateway("the upstream metadata cannot be read"))?;
                // Un proxy no ofrece versiones que su politica no admite: el
                // cliente las pediria y recibiria un 404.
                let config = the_config(member)?;
                declared
                    .versions
                    .retain(|version| config.admits(version.ends_with(maven::SNAPSHOT_SUFFIX)));
                declared
            }
            MetadataLevel::Version { .. } => maven::Contribution::default(),
        };
        return Ok(Some(Piece {
            bytes,
            contribution,
            verbatim: true,
        }));
    }
    // Un hosted aporta lo que tiene publicado.
    match level {
        MetadataLevel::Artifact {
            group_id,
            artifact_id,
        } => {
            let prefix = format!("{}/{artifact_id}/", group_id.replace('.', "/"));
            let known = known_under(state, context, member, &prefix).await?;
            let contribution = maven::hosted_contribution(group_id, artifact_id, &known);
            if contribution.versions.is_empty() {
                return Ok(None);
            }
            Ok(Some(Piece {
                bytes: Vec::new(),
                contribution,
                verbatim: false,
            }))
        }
        MetadataLevel::Version { .. } => Ok(derived_metadata(state, context, member, level)
            .await?
            .map(|xml| Piece {
                bytes: xml.into_bytes(),
                contribution: maven::Contribution::default(),
                verbatim: false,
            })),
    }
}

/// Los metadatos que sirve un grupo: de artefacto, la union de las
/// versiones de todos los miembros; de version, los del primer miembro que
/// los tiene.
///
/// Devuelve los bytes y los miembros que aportaron. El rastro queda en
/// `trail`.
async fn group_metadata(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    level: &MetadataLevel,
    metadata_path: &str,
    trail: &mut Vec<(String, super::raw_group::Step)>,
    correlation: &str,
) -> Result<(Vec<u8>, Vec<String>), RawApiError> {
    use super::raw_group::Step;

    let mut pieces: Vec<(String, Piece)> = Vec::new();
    // Un proxy sin red y sin copia no tiene nada que aportar: se sigue con
    // el siguiente. Si al final nadie sirvio, ese es el error.
    let mut offline: Option<RawApiError> = None;
    // La reserva de namespaces del grupo se consulta antes de preguntar a
    // sus miembros: los que dejo fuera quedan en el rastro, y sus upstreams
    // no aportan versiones aunque tengan mas.
    let (leaves, denied, judgement) =
        super::raw_group::the_governed_leaves(state, context, group, metadata_path).await?;
    for key in &denied {
        trail.push((key.clone(), Step::Deny));
    }
    for member in leaves {
        let key = member.repo_key.clone();
        match the_piece(state, context, &member, level, metadata_path, correlation).await {
            Ok(Some(piece)) => {
                trail.push((key.clone(), Step::Hit));
                pieces.push((key, piece));
                // Los de version no se fusionan: gana el primero.
                if matches!(level, MetadataLevel::Version { .. }) {
                    break;
                }
            }
            Ok(None) => trail.push((key, Step::Miss)),
            Err(error @ RawApiError::Offline(_)) => {
                trail.push((key, Step::Miss));
                offline = Some(error);
            }
            Err(error) => {
                trail.push((key, Step::Error));
                return Err(error);
            }
        }
    }
    if pieces.is_empty()
        && let Some(error) = offline
    {
        return Err(error);
    }
    if pieces.is_empty()
        && !denied.is_empty()
        && let Some(judgement) = judgement
    {
        return Err(super::raw_group::the_group_reservation_explanation(
            group, &judgement, &denied,
        ));
    }
    let MetadataLevel::Artifact {
        group_id,
        artifact_id,
    } = level
    else {
        let (key, piece) = pieces.into_iter().next().ok_or(RawApiError::NotFound)?;
        return Ok((piece.bytes, vec![key]));
    };

    let contributions: Vec<maven::Contribution> = pieces
        .iter()
        .map(|(_, piece)| piece.contribution.clone())
        .collect();
    let contributors = |only_versions: bool| -> Vec<String> {
        pieces
            .iter()
            .filter(|(_, piece)| !only_versions || !piece.contribution.versions.is_empty())
            .map(|(key, _)| key.clone())
            .collect()
    };
    match maven::merged_metadata(group_id, artifact_id, &contributions) {
        Some(xml) => Ok((xml.into_bytes(), contributors(true))),
        None => {
            // Metadatos que no listan versiones (por ejemplo los de un
            // grupo de plugins): no se fusionan, se sirve el primero que
            // un upstream trae, tal cual.
            let (key, piece) = pieces
                .into_iter()
                .find(|(_, piece)| {
                    piece.verbatim && maven::lists_no_versions(&piece.bytes, group_id, artifact_id)
                })
                .ok_or(RawApiError::NotFound)?;
            Ok((piece.bytes, vec![key]))
        }
    }
}

/// El checksum de un fichero que sirve un grupo: es el del fichero del
/// miembro que gana esa ruta, nunca el de un sidecar de otro miembro.
async fn group_file_checksum(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    subject_path: &str,
    algorithm: Algorithm,
    trail: &mut Vec<(String, super::raw_group::Step)>,
    correlation: &str,
) -> Result<(Vec<u8>, Vec<String>), RawApiError> {
    let resolution =
        super::raw_group::the_group_asset(state, context, group, subject_path, correlation).await;
    trail.extend(resolution.trail.iter().cloned());
    // El recorrido del grupo ya aplico la reserva de namespaces: los
    // miembros que dejo fuera aparecen en el rastro como `deny`.
    let won = resolution.outcome?;
    crate::trust::admit(
        state,
        context.account().as_str(),
        &won.member,
        &won.asset.digest,
    )
    .await
    .map_err(the_admission_error)?;
    let text = checksum_of(state, &won.asset, algorithm).await?;
    Ok((text.into_bytes(), vec![won.member.repo_key]))
}

/// Lo que un grupo Maven sirve en una ruta de metadatos o de checksum.
///
/// `Ok(None)` dice que la ruta es de un fichero: la resuelve el grupo como
/// cualquier otro (gana el primer miembro que lo tiene).
pub(super) async fn the_group_derived(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    asset_path: &str,
    headers: &HeaderMap,
    head: bool,
    correlation: &str,
) -> Result<Option<Response>, RawApiError> {
    let Ok(target) = maven::parse_path(asset_path) else {
        return Ok(None);
    };
    let mut trail: Vec<(String, super::raw_group::Step)> = Vec::new();
    let outcome = match target {
        Target::File(_) => return Ok(None),
        Target::Metadata(level) => {
            let found = group_metadata(
                state,
                context,
                group,
                &level,
                asset_path,
                &mut trail,
                correlation,
            )
            .await;
            found.map(|(bytes, from)| (bytes, "application/xml", from))
        }
        Target::Checksum {
            algorithm,
            subject: Subject::Metadata(level),
            subject_path,
        } => {
            let found = group_metadata(
                state,
                context,
                group,
                &level,
                &subject_path,
                &mut trail,
                correlation,
            )
            .await;
            found.map(|(bytes, from)| {
                (
                    checksum_of_bytes(&bytes, algorithm).into_bytes(),
                    "text/plain; charset=utf-8",
                    from,
                )
            })
        }
        Target::Checksum {
            algorithm,
            subject: Subject::File(_),
            subject_path,
        } => {
            let found = group_file_checksum(
                state,
                context,
                group,
                &subject_path,
                algorithm,
                &mut trail,
                correlation,
            )
            .await;
            found.map(|(bytes, from)| (bytes, "text/plain; charset=utf-8", from))
        }
    };
    super::raw_group::record_resolution(
        state,
        context,
        group,
        asset_path,
        &super::raw_group::trail_text(&trail),
        outcome.is_ok(),
        correlation,
    );
    let (content, content_type, from) = outcome?;
    let trace = GroupTrace {
        from: Some(from.join(",")),
        version: Some(group.config.version),
    };
    respond_derived(content, content_type, headers, head, correlation, &trace).map(Some)
}
