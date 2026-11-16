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
use hodei_catalog::{AssetRecord, CatalogError, RepositoryDescriptor};
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
            return Ok(Some(response));
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
    Ok(Some(response))
}
