//! Resolution of a group repository on the OCI surface.
//!
//! An OCI group stores nothing: it is an **ordered list** of repositories
//! of the same format, and precedence is given by the order in which they are
//! configured. The first member that has the reference wins; there is no
//! merging, no voting, and no "the most recent": two reads of the same
//! reference with the same configuration give the same source.
//!
//! # MISS and ERROR are not the same
//!
//! - **MISS**: the member says with certainty that it does not have it (it has no such
//!   tag, or its asset row is not there). Resolution continues with the next one.
//! - **ERROR**: anything else (the catalog does not respond, the trust
//!   ledger fails, the bytes are not where the row says they are). Resolution
//!   **stops** and that error is returned. Skipping to the next
//!   member would serve content that perhaps is not what the first one would have
//!   given: a transient failure would silently change what is downloaded.
//!
//! # Coherence and provenance
//!
//! A manifest is served **whole** from a single member, and blobs are
//! addressed by their digest: they are only served after checking that the
//! bytes **are** the requested digest. Every group response says which
//! member served it (`X-Hodei-Resolved-From`) and in which version of the
//! group configuration it was resolved (`X-Hodei-Group-Version`).
//!
//! # Permissions and trust
//!
//! Cedar decides on the group **and on each member**: a member the
//! principal cannot read is omitted without saying it exists. The trust
//! gate is applied to the digest of the **member that won**: if it hides it, the
//! result is a "does not exist" and the group does **not** move on to the next member,
//! because then the revocation of a digest would make other content appear
//! in its place.

use std::collections::HashMap;

use axum::http::HeaderMap;
use hodei_authz::VerifiedContext;
use hodei_catalog::{
    CatalogError, GroupConfig, GroupError, Lookup, Node, RepositoryDescriptor, RepositoryMode,
};
use hodei_evidence::ContentDigest;

use crate::app_state::AppState;
use crate::oci::failure::OciFailure;
use crate::oci::target::the_digest_of_reference;

/// What a group resolution won.
pub struct Won {
    /// The leaf member that served the content.
    pub member: RepositoryDescriptor,
    /// The digest of the served content.
    pub digest: ContentDigest,
    /// The bytes of the content.
    pub bytes: Vec<u8>,
}

/// How the query to a member ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The member had the reference.
    Hit,
    /// The member said with certainty that it does not have it.
    Miss,
    /// The member failed: resolution stopped there.
    Error,
}

impl Step {
    /// The step, in one line.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Error => "error",
        }
    }
}

/// The resolution: what won (or the error) and the order of what was queried.
pub struct Resolution {
    /// The result.
    pub outcome: Result<Won, OciFailure>,
    /// Each queried member, in order, with its result.
    pub trail: Vec<(String, Step)>,
}

impl Resolution {
    /// The trail in one line: `first:miss,second:hit`.
    pub fn trail_line(&self) -> String {
        self.trail
            .iter()
            .map(|(member, step)| format!("{member}:{}", step.as_str()))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// A group configuration error, translated to the distribution
/// dialect.
///
/// Members were validated when the group was saved; if they no longer add up (
/// another group was reconfigured, a member no longer exists) the group is badly
/// configured and is not resolved halfway.
fn the_group_failure(error: GroupError) -> OciFailure {
    tracing::warn!(error = %error, "an oci group does not resolve");
    OciFailure::unavailable("the group is not well configured")
}

/// Whether the principal can read this repository. It is the Cedar decision
/// on its HRN, the same one its direct route applies.
fn can_read(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
) -> Result<bool, OciFailure> {
    state
        .authorize_resource
        .execute_repository(
            context.clone(),
            crate::actions::download_asset(),
            repository,
        )
        .map(|outcome| outcome.allows_access())
        .map_err(|_| OciFailure::unavailable("the authorization engine is not available"))
}

/// Reads the repositories the group reaches and says which ones the
/// principal can read.
async fn the_members(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    config: &GroupConfig,
) -> Result<
    (
        HashMap<String, Lookup>,
        HashMap<String, RepositoryDescriptor>,
    ),
    OciFailure,
> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    let account = context.account().as_str();
    let mut known: HashMap<String, Lookup> = HashMap::new();
    let mut found: HashMap<String, RepositoryDescriptor> = HashMap::new();
    let mut pending: Vec<String> = config.members.clone();
    while let Some(key) = pending.pop() {
        if key == group.repo_key || known.contains_key(&key) {
            continue;
        }
        if known.len() >= hodei_catalog::group::MAX_NODES {
            return Err(the_group_failure(GroupError::TooManyNodes));
        }
        let lookup = match reader.repository(account, &key).await {
            // A member of another account does not exist for this one: it is omitted
            // as if it did not exist, without saying it exists elsewhere.
            Err(CatalogError::Missing { .. }) => Lookup::Missing,
            Err(CatalogError::Denied { .. }) => Lookup::Hidden,
            Err(_) => return Err(OciFailure::unavailable("the catalog is not available")),
            Ok(descriptor) => {
                if !can_read(state, context, &descriptor)? {
                    Lookup::Hidden
                } else {
                    let members = if descriptor.mode == RepositoryMode::Group {
                        match GroupConfig::of(&descriptor.config) {
                            Ok(Some(nested)) => nested.members,
                            _ => {
                                return Err(OciFailure::unavailable(
                                    "the group is not well configured",
                                ));
                            }
                        }
                    } else {
                        Vec::new()
                    };
                    pending.extend(members.iter().cloned());
                    let node = Node {
                        format: descriptor.format,
                        mode: descriptor.mode,
                        members,
                    };
                    found.insert(key.clone(), descriptor);
                    Lookup::Found(node)
                }
            }
        };
        known.insert(key, lookup);
    }
    Ok((known, found))
}

/// The leaf members of a group, in the order they are queried, already
/// read and filtered by what the principal can read.
pub async fn the_leaves(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
) -> Result<Vec<RepositoryDescriptor>, OciFailure> {
    let config = match GroupConfig::of(&group.config) {
        Ok(Some(config)) => config,
        _ => {
            tracing::warn!(group = %group.repo_key, "an oci group has no readable members");
            return Err(OciFailure::unavailable("the group is not well configured"));
        }
    };
    let (known, mut found) = the_members(state, context, group, &config).await?;
    let walk = hodei_catalog::walk_group(&group.repo_key, group.format, &config.members, &|key| {
        known.get(key).cloned().unwrap_or(Lookup::Missing)
    })
    .map_err(the_group_failure)?;
    walk.leaves
        .iter()
        .map(|key| {
            found
                .remove(key)
                .ok_or_else(|| OciFailure::unavailable("the catalog is not available"))
        })
        .collect()
}

/// What a group lets through from its leaves for **one path**: the leaves that
/// are queried and the proxy members that the group's namespace
/// reservation denied.
///
/// The reservation lives in the group configuration and is always consulted,
/// before looking at what each member has: a reserved path is not served
/// from any public upstream of the group, and a down upstream does not
/// turn it into a fallback. Hosted members are the internal source and
/// the reservation does not touch them.
///
/// The judgment is on the member's **repository name**: in OCI the
/// namespace is the repository name (`team/demo`), not the asset
/// path (`manifests/<digest>`). A judgment on the asset path was
/// inert: the first segment of the path is always `manifests`, `blobs` or
/// `tags`, and would only match a reservation called that.
pub async fn the_governed_leaves(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
) -> Result<(Vec<RepositoryDescriptor>, Vec<String>), OciFailure> {
    let leaves = the_leaves(state, context, group).await?;
    let policy = hodei_catalog::NamespacePolicy::of(&group.config).map_err(|error| {
        tracing::warn!(error = %error, "the namespace policy of a group cannot be read");
        OciFailure::unavailable("the group is not well configured")
    })?;
    let Some(policy) = policy else {
        return Ok((leaves, Vec::new()));
    };
    let mut consulted = Vec::with_capacity(leaves.len());
    let mut denied = Vec::new();
    for member in leaves {
        // The reservation is judged by the member's repository name.
        let judgement = policy.judge(group.format, &member.repo_key);
        if judgement.is_reserved() && member.mode == RepositoryMode::Proxy {
            denied.push(member.repo_key.clone());
        } else {
            consulted.push(member);
        }
    }
    Ok((consulted, denied))
}

/// Whether a member's failure is a **MISS**.
///
/// A MISS is what the member says with certainty it does not have: a
/// manifest or a blob that do not exist. Everything else --a down upstream, a
/// catalog that does not respond, a trust ledger that cannot be read-- is an
/// ERROR, and the ERROR **stops** resolution: a transient failure cannot
/// silently change what is downloaded.
fn is_a_miss(failure: &OciFailure) -> bool {
    matches!(failure.the_code(), "MANIFEST_UNKNOWN" | "BLOB_UNKNOWN")
}

/// The digest a reference points to in a member.
///
/// A reference with an algorithm is a digest; a reference without it is a
/// tag, and the tag is the mutable reference the catalog
/// resolves to the asset's coordinates.
async fn the_digest_of(
    reader: &dyn hodei_catalog::CatalogReader,
    account: &str,
    member: &str,
    reference: &str,
) -> Result<String, OciFailure> {
    if reference.contains(':') {
        return the_digest_of_reference(reference);
    }
    let tag = reader
        .tag(account, member, reference)
        .await
        .map_err(|error| match error {
            // The member says with certainty it does not have that tag: it is a
            // MISS, and resolution continues with the next one.
            CatalogError::Missing { .. } => OciFailure::manifest_unknown("unknown"),
            _other => OciFailure::unavailable("the catalog is not available"),
        })?
        .coordinates
        .package_version;
    the_digest_of_reference(&format!("sha256:{tag}"))
}

/// The asset row that serves a member's content.
///
/// `Ok(None)` is a MISS: the member does not have that reference. An error is
/// an ERROR: resolution stops.
async fn the_asset(
    reader: &dyn hodei_catalog::CatalogReader,
    account: &str,
    member: &str,
    asset_path: &str,
) -> Result<Option<hodei_catalog::AssetRecord>, OciFailure> {
    match reader.asset(account, member, asset_path).await {
        Ok(asset) => Ok(Some(asset)),
        Err(CatalogError::Missing { .. }) => Ok(None),
        Err(_) => Err(OciFailure::unavailable("the catalog is not available")),
    }
}

/// The asset row of the requested digest.
///
/// The row says **what** there is and the requested digest says **what** is asked for: if they do
/// not match, the member does not have what was asked for, and it is a MISS.
fn the_asset_of(
    asset: hodei_catalog::AssetRecord,
    digest: &str,
) -> Option<hodei_catalog::AssetRecord> {
    if asset.digest.value == digest {
        Some(asset)
    } else {
        None
    }
}

/// The bytes the row references.
///
/// The URL digest says **what** is asked for; the row says **what** there is. If
/// the bytes of the URL digest were served, a row that only names
/// a path would serve the content of another blob, which may belong to another
/// account: the CAS is a shared tree.
async fn the_bytes(
    state: &AppState,
    asset: &hodei_catalog::AssetRecord,
) -> Result<Vec<u8>, OciFailure> {
    let store = crate::oci::the_content_store(state)?;
    store
        .get(&asset.digest)
        .await
        .map_err(|_| OciFailure::unavailable("the catalog and the store disagree"))
}

/// A manifest of the group, by digest or by tag.
///
/// It walks the members in the configured order and the first one that
/// has the reference wins. A MISS continues to the next; an ERROR stops
/// resolution.
pub async fn the_manifest(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    reference: &str,
    headers: &HeaderMap,
) -> Resolution {
    let mut trail: Vec<(String, Step)> = Vec::new();
    let outcome = resolve_manifest(state, context, group, reference, headers, &mut trail).await;
    Resolution { outcome, trail }
}

async fn resolve_manifest(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    reference: &str,
    headers: &HeaderMap,
    trail: &mut Vec<(String, Step)>,
) -> Result<Won, OciFailure> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    let account = context.account().as_str();

    // The reservation is consulted before the members: a reserved path is not
    // served from the group's public members.
    let (leaves, _denied) = the_governed_leaves(state, context, group).await?;

    for member in leaves {
        let key = member.repo_key.clone();
        // A **proxy** member has no rows in the catalog: what it has
        // is resolved by its upstream, and it is resolved with the **member's
        // name**: the group does not rename its member.
        if member.mode == RepositoryMode::Proxy {
            let resolved = match crate::oci::proxy::the_resolved_manifest(
                state, context, &member, &key, reference, headers,
            )
            .await
            {
                Ok(resolved) => resolved,
                Err(failure) => {
                    // An upstream that says with certainty it does not have it is
                    // a MISS, and resolution continues. A down upstream is an ERROR, and
                    // resolution stops: it does not skip to the next
                    // member, because the failure would silently change what
                    // is downloaded.
                    if is_a_miss(&failure) {
                        trail.push((key, Step::Miss));
                        continue;
                    }
                    trail.push((key, Step::Error));
                    return Err(failure);
                }
            };
            // The trust gate is applied to the digest of the **member
            // that won**: if it hides it, the result is a "does not exist" and the
            // group does **not** move on to the next member.
            if let Err(admission) =
                crate::trust::admit(state, account, &member, &resolved.asset.digest).await
            {
                trail.push((key, Step::Error));
                return Err(match admission {
                    crate::trust::Admission::Hidden => OciFailure::manifest_unknown(format!(
                        "the manifest {reference:?} is unknown"
                    )),
                    crate::trust::Admission::Unavailable => {
                        OciFailure::unavailable("the trust ledger is not available")
                    }
                });
            }
            // The bytes are read **after** the gate: what cannot
            // be served is not materialized.
            let bytes = match crate::oci::proxy::the_bytes(state, &resolved.asset).await {
                Ok(bytes) => bytes,
                Err(failure) => {
                    trail.push((key, Step::Error));
                    return Err(failure);
                }
            };
            trail.push((key.clone(), Step::Hit));
            return Ok(Won {
                member,
                digest: resolved.asset.digest,
                bytes,
            });
        }
        // The digest is resolved **within** the member's namespace: each
        // member has its own tags and its own digests.
        let digest = match the_digest_of(reader.as_ref(), account, &key, reference).await {
            Ok(digest) => digest,
            Err(failure) => {
                // The tag error may be a MISS (the member does not
                // have that tag) or an ERROR (the catalog does not respond).
                // The dialect says so: `manifest_unknown` is a MISS, and
                // anything else stops resolution.
                if failure.the_code() == "MANIFEST_UNKNOWN" {
                    trail.push((key, Step::Miss));
                    continue;
                }
                trail.push((key, Step::Error));
                return Err(failure);
            }
        };
        let asset = match the_asset(
            reader.as_ref(),
            account,
            &key,
            &format!("manifests/{digest}"),
        )
        .await
        {
            Ok(Some(asset)) => the_asset_of(asset, &digest),
            Ok(None) => None,
            Err(failure) => {
                trail.push((key, Step::Error));
                return Err(failure);
            }
        };
        let Some(asset) = asset else {
            trail.push((key, Step::Miss));
            continue;
        };
        // The trust gate is applied to the digest of the **member that
        // won**: if it hides it, the next member is not searched, because
        // then the revocation of a digest would make other content appear
        // in its place.
        if let Err(admission) = crate::trust::admit(state, account, &member, &asset.digest).await {
            trail.push((key, Step::Error));
            return Err(match admission {
                crate::trust::Admission::Hidden => {
                    OciFailure::manifest_unknown(format!("the manifest {reference:?} is unknown"))
                }
                crate::trust::Admission::Unavailable => {
                    OciFailure::unavailable("the trust ledger is not available")
                }
            });
        }
        let bytes = match the_bytes(state, &asset).await {
            Ok(bytes) => bytes,
            Err(failure) => {
                trail.push((key, Step::Error));
                return Err(failure);
            }
        };
        trail.push((key.clone(), Step::Hit));
        return Ok(Won {
            member,
            digest: ContentDigest::sha256(&digest),
            bytes,
        });
    }
    Err(OciFailure::manifest_unknown(format!(
        "the manifest {reference:?} is unknown"
    )))
}

/// A blob of the group, by its digest.
///
/// The blob is addressed by digest: it is only served after checking that the
/// bytes **are** the requested digest.
pub async fn the_blob(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    digest: &str,
    headers: &HeaderMap,
) -> Resolution {
    let mut trail: Vec<(String, Step)> = Vec::new();
    let outcome = resolve_blob(state, context, group, digest, headers, &mut trail).await;
    Resolution { outcome, trail }
}

async fn resolve_blob(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    digest: &str,
    headers: &HeaderMap,
    trail: &mut Vec<(String, Step)>,
) -> Result<Won, OciFailure> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    let account = context.account().as_str();

    let (leaves, _denied) = the_governed_leaves(state, context, group).await?;

    for member in leaves {
        let key = member.repo_key.clone();
        // A **proxy** member resolves the blob in its upstream, with the
        // member's name and not the group's.
        if member.mode == RepositoryMode::Proxy {
            let resolved = match crate::oci::proxy::the_resolved_blob(
                state, context, &member, &key, digest, headers,
            )
            .await
            {
                Ok(resolved) => resolved,
                Err(failure) => {
                    if is_a_miss(&failure) {
                        trail.push((key, Step::Miss));
                        continue;
                    }
                    trail.push((key, Step::Error));
                    return Err(failure);
                }
            };
            if let Err(admission) =
                crate::trust::admit(state, account, &member, &resolved.asset.digest).await
            {
                trail.push((key, Step::Error));
                return Err(match admission {
                    crate::trust::Admission::Hidden => OciFailure::blob_unknown(format!(
                        "the repository does not reference {digest}"
                    )),
                    crate::trust::Admission::Unavailable => {
                        OciFailure::unavailable("the trust ledger is not available")
                    }
                });
            }
            // The bytes are read **after** the gate: what cannot
            // be served is not materialized.
            let bytes = match crate::oci::proxy::the_bytes(state, &resolved.asset).await {
                Ok(bytes) => bytes,
                Err(failure) => {
                    trail.push((key, Step::Error));
                    return Err(failure);
                }
            };
            trail.push((key.clone(), Step::Hit));
            return Ok(Won {
                member,
                digest: resolved.asset.digest,
                bytes,
            });
        }
        let asset =
            match the_asset(reader.as_ref(), account, &key, &format!("blobs/{digest}")).await {
                Ok(Some(asset)) => the_asset_of(asset, digest),
                Ok(None) => None,
                Err(failure) => {
                    trail.push((key, Step::Error));
                    return Err(failure);
                }
            };
        let Some(asset) = asset else {
            trail.push((key, Step::Miss));
            continue;
        };
        if let Err(admission) = crate::trust::admit(state, account, &member, &asset.digest).await {
            trail.push((key, Step::Error));
            return Err(match admission {
                crate::trust::Admission::Hidden => {
                    OciFailure::blob_unknown(format!("the repository does not reference {digest}"))
                }
                crate::trust::Admission::Unavailable => {
                    OciFailure::unavailable("the trust ledger is not available")
                }
            });
        }
        let bytes = match the_bytes(state, &asset).await {
            Ok(bytes) => bytes,
            Err(failure) => {
                trail.push((key, Step::Error));
                return Err(failure);
            }
        };
        trail.push((key.clone(), Step::Hit));
        return Ok(Won {
            member,
            digest: ContentDigest::sha256(digest),
            bytes,
        });
    }
    Err(OciFailure::blob_unknown(format!(
        "the repository does not reference {digest}"
    )))
}

/// The tags of the group: the **union** of those of its members.
///
/// One entry per name: if two members have the same tag, the one
/// from the first in the order of precedence is the one that counts, and the one that
/// is resolved on download. And the group listing **is not served partially**: if
/// a member's list cannot be obtained, the whole listing is an
/// error, because a half list looks complete and is not.
///
/// A **proxy** member contributes the tags of its **upstream**: in a proxy
/// the tag is a pointer, not a catalog row, so the only
/// list there is is the upstream's. It is asked with the **member's** name,
/// which is the name the upstream knows.
pub async fn the_tags(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    headers: &HeaderMap,
) -> Result<Vec<(String, String)>, OciFailure> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    let account = context.account().as_str();
    let (leaves, _denied) = the_governed_leaves(state, context, group).await?;
    let mut names: Vec<(String, String)> = Vec::new();
    for member in leaves {
        // A **proxy** member has no tag rows in the catalog: its
        // list lives in the upstream, and is asked with the member's
        // name.
        if member.mode == RepositoryMode::Proxy {
            let listed = crate::oci::proxy::the_resolved_tags(
                state,
                &member,
                &member.repo_key,
                headers,
            )
            .await
                        // The group listing **is not served partially**: if a member's
                        // list cannot be obtained --a down upstream, a credential
                        // that cannot be used-- the whole listing is an error. A
                        // half list looks complete and is not.
            ?;
            let crate::oci::proxy::TheUpstreamTags::Listed(tags) = listed else {
                // The upstream cannot be asked for its list: it is a
                // member that **cannot enumerate**, not a failure. It contributes no
                // tags and the other members are listed anyway.
                continue;
            };
            // The trust filter cannot be applied per tag: the
            // upstream's list is names, not digests. The gate is
            // applied to the **content**, at resolution, which is where it is
            // decided whether the bytes can be served.
            for tag in tags {
                // One entry per name: the first member that has the
                // tag is the one that wins, and the one that resolves it on
                // download.
                if !names.iter().any(|(name, _)| *name == tag) {
                    names.push((tag, member.repo_key.clone()));
                }
            }
            continue;
        }
        let tags = reader
            .tags(account, &member.repo_key)
            .await
            .map_err(|_| OciFailure::unavailable("the catalog is not available"))?;
        // The trust filter is per member: a tag towards an
        // ungranted digest would reveal that it exists.
        let servable = crate::trust::servable(state, account, &member)
            .await
            .map_err(|_| OciFailure::unavailable("the trust ledger is not available"))?;
        for tag in tags {
            if let Some(digests) = &servable
                && !digests.contains(&tag.coordinates.package_version)
            {
                continue;
            }
            // One entry per name: the first member that has the
            // tag is the one that wins.
            if !names.iter().any(|(name, _)| *name == tag.tag_name) {
                names.push((tag.tag_name, member.repo_key.clone()));
            }
        }
    }
    Ok(names)
}
