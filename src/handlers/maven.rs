//! Maven 2 hosted over the content surface.
//!
//! A hosted Maven 2 repository uses the same routes and the same model as
//! the raw one (`/repository/{repo}/{*path}`): each file is an asset whose
//! content lives in the store by digest. What changes is what is
//! decided **before** storing and **when** serving what Maven calls
//! metadata:
//!
//! - **Writing.** The path must be a valid Maven 2 path, the version
//!   must fit the repository's policy, and replacement is not
//!   decided by the client: a release does not change (except `release_redeploy`), a
//!   timestamped snapshot does not change, and the floating name
//!   `-SNAPSHOT` is replaced. A POM is read before storing it and its
//!   coordinates must be those of its path.
//! - **`maven-metadata.xml` and checksums.** They are not stored: they are **derived** from the
//!   catalog every time they are requested. This way there is no metadata file that
//!   can be left half-written, nor two concurrent deployments overwriting it:
//!   what is listed is what is published, and a version appears when
//!   its POM exists. What a client uploads under those names is **validated** and
//!   accepted without storing, so that `mvn deploy` ends the same as against
//!   any other repository.
//! - **File checksums.** They are compared with the digest computed from the
//!   stored content: if the client uploads one that is not the file's, it is
//!   rejected; if it is, it is accepted (the server already knows it) without storing it.

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

/// How much is read from the store at once when computing a digest: a one-gigabyte
/// jar does not fit in memory.
const HASH_CHUNK: u64 = 4 * 1024 * 1024;

/// What a write asks to do.
pub(super) enum Write {
    /// A file of the artifact: it is stored.
    File {
        /// The file, already analyzed.
        file: MavenFile,
        /// Whether its address can be replaced.
        replace: ReplacePolicy,
    },
    /// Metadata or checksum: validated and not stored.
    Absorbed(Absorbed),
}

/// What is validated and accepted without storing.
pub(super) enum Absorbed {
    /// `maven-metadata.xml`.
    Metadata(MetadataLevel),
    /// A checksum of `maven-metadata.xml`.
    MetadataChecksum(Algorithm),
    /// A checksum of a file of the artifact.
    FileChecksum {
        /// The algorithm.
        algorithm: Algorithm,
        /// The path of the file it is the checksum of.
        subject_path: String,
    },
}

/// The Maven configuration of a repository.
fn the_config(repository: &RepositoryDescriptor) -> Result<MavenConfig, RawApiError> {
    // Validated when the repository is saved; if it is not understood now, the
    // repository is broken and is not served halfway.
    MavenConfig::of_settings(&repository.config.config_settings).map_err(|error| {
        tracing::warn!(error = %error, "the maven settings of a repository cannot be read");
        RawApiError::Unavailable
    })
}

/// What must be done with a write at a path of a Maven 2 repository.
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
                // The floating name is the one a snapshot replaces.
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

/// Reads a whole body with a cap.
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

/// The body of a file, ready to store.
///
/// A POM is read whole before storing it: its coordinates must be
/// those of its path, and a POM that is not leaves not a single byte in the store.
/// The rest of the files pass through in chunks, untouched.
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

/// The digest of stored content, read in chunks.
async fn checksum_of(
    state: &AppState,
    asset: &AssetRecord,
    algorithm: Algorithm,
) -> Result<String, RawApiError> {
    if algorithm == Algorithm::Sha256 {
        // The content's digest is already its SHA-256.
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
            // A store that returns less than the catalog says is
            // broken: a digest is not invented.
            return Err(RawApiError::Internal);
        }
        hasher.update(&chunk);
        offset += chunk.len() as u64;
    }
    Ok(hasher.finish())
}

/// A digest in progress.
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

/// The digest of some in-memory bytes.
fn checksum_of_bytes(bytes: &[u8], algorithm: Algorithm) -> String {
    let mut hasher = Hasher::of(algorithm);
    hasher.update(bytes);
    hasher.finish()
}

/// Validates what a client uploads under a metadata or checksum name, and
/// accepts it without storing it.
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

/// What is published under a prefix, with the digests the trust gate
/// lets be served.
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

/// The metadata derived from what is published, if there is anything to describe.
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

/// What is served at a metadata or checksum path.
///
/// `Ok(None)` says the path is not a metadata or checksum path: the normal
/// file flow serves it.
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
                    // A file's digest belongs to the file: the trust
                    // gate decides on its digest, just as when
                    // downloading it.
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

/// What a group adds to a derived response: who contributed the content
/// and in which version of its configuration.
#[derive(Default)]
pub(super) struct GroupTrace {
    /// The members that contributed, separated by commas.
    pub from: Option<String>,
    /// The version of the group's configuration.
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

/// The response of derived content: conditionals, headers and body.
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
    // Derived from what is published: whoever stores it must revalidate it.
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response_headers.insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// Groups and proxies.
// ---------------------------------------------------------------------------

/// Whether the version policy of a Maven 2 repository admits the path.
///
/// A proxy applies the same policy as a hosted one (`release` / `snapshot`):
/// a version the repository does not admit is not requested from the upstream. What
/// is neither a file nor version metadata is let through: the proxy does not
/// invent rules about paths it does not understand.
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
        // What is not understood passes, unless it hangs under a `-SNAPSHOT`
        // folder: there a strange name is not a way of dodging the
        // policy.
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

/// What a leaf member of a group contributes to metadata.
struct Piece {
    /// The bytes the member serves at that metadata path.
    bytes: Vec<u8>,
    /// What they contribute to the artifact's version list.
    contribution: maven::Contribution,
    /// Whether an upstream serves it as is (and it is not derived from what is published).
    verbatim: bool,
}

/// Reads a metadata file a proxy has, with a size cap.
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

/// What a leaf member contributes to the metadata of `level`.
///
/// `Ok(None)` is a MISS: the member has no metadata for that path, or the
/// trust gate hides the file that backs it.
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
            // The artifact ones are a union: a member that hides them does not
            // contribute. The version ones are served by a single member: hidden is
            // a 404 and the next one is not searched, as with a file.
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
                // A proxy does not offer versions its policy does not admit: the
                // client would request them and receive a 404.
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
    // A hosted one contributes what it has published.
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

/// The metadata a group serves: for the artifact, the union of the
/// versions of all members; for the version, those of the first member that
/// has them.
///
/// Returns the bytes and the members that contributed. The trail is left in
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
    // A proxy without network and without a copy has nothing to contribute: the next
    // one is tried. If in the end nobody served, that is the error.
    let mut offline: Option<RawApiError> = None;
    // The group's namespace reservation is consulted before asking its
    // members: those it left out remain in the trail, and their upstreams
    // do not contribute versions even if they have more.
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
                // The version ones are not merged: the first one wins.
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
            // Metadata that does not list versions (for example that of a
            // plugin group): it is not merged, the first one an
            // upstream brings is served, as is.
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

/// The checksum of a file served by a group: it is that of the file of the
/// member that wins that path, never that of another member's sidecar.
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
    // The group traversal already applied the namespace reservation: the
    // members it left out appear in the trail as `deny`.
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

/// What a Maven group serves at a metadata or checksum path.
///
/// `Ok(None)` says the path is a file path: the group resolves it like
/// any other (the first member that has it wins).
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
