//! The resolution of a group repository on the RAW surface.
//!
//! A group stores nothing: it is an ordered list of repositories (hosted,
//! proxy or other groups). Reading a path of the group walks the members in
//! order and the first one that has it wins.
//!
//! # MISS and ERROR are not the same
//!
//! - **MISS**: the member says with certainty that it does not have the path (a hosted one
//!   without that asset; a proxy whose upstream answers 404, or that has it
//!   remembered as absent). The next one is tried.
//! - **ERROR**: anything else (the catalog does not respond, the upstream
//!   fails or times out, the scope is not served...). The resolution
//!   stops and that error is answered. Jumping to the next member would serve
//!   content that perhaps is not what the first would have given: a
//!   transient failure would silently change what is downloaded.
//!
//! # Permissions
//!
//! Cedar decides on each repository: the group (`DownloadAsset` on its
//! HRN, in `the_content_of`) and **each member** (here). A member the
//! principal cannot read is omitted from the resolution, without saying it exists:
//! the group does not grant access to what the principal could not read via its
//! direct path, and the direct path inherits nothing from the group.
//!
//! The trust gate is applied afterwards, on the member that won
//! and its digest. If the gate hides it, the result is a 404: the
//! next member is not searched, because then the revocation of a
//! digest would make other content appear in its place.

use std::collections::HashMap;

use hodei_authz::VerifiedContext;
use hodei_catalog::{
    AssetRecord, CatalogError, GroupConfig, GroupError, Lookup, Node, RepositoryDescriptor,
    RepositoryMode,
};

use crate::app_state::AppState;
use crate::resolution::{
    CacheStatus, ProxyServed, ResolutionError as RawApiError, the_current_asset,
    the_namespace_policy, the_proxy_asset, the_reservation_explanation,
};

/// How the query to a member ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// The member had the path.
    Hit,
    /// The member said with certainty that it does not have it.
    Miss,
    /// The member was not queried: the group's namespace reservation
    /// leaves it out for this path. It stays in the trail because the refusal is
    /// explained, not hidden.
    Deny,
    /// The member failed: the resolution stopped there.
    Error,
}

impl Step {
    fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Deny => "deny",
            Self::Error => "error",
        }
    }
}

/// What a group resolution won.
pub(crate) struct Won {
    /// The leaf member that had the path.
    pub member: RepositoryDescriptor,
    /// What the member serves at that path.
    pub asset: AssetRecord,
    /// How it was served, if the member is a proxy.
    pub served: Option<(CacheStatus, Option<(String, String)>)>,
}

/// The resolution: what won (or the error) and the order of what was queried.
pub(crate) struct Resolution {
    /// The result.
    pub outcome: Result<Won, RawApiError>,
    /// Each member queried, in order, with its result.
    pub trail: Vec<(String, Step)>,
}

impl Resolution {
    /// The trail on one line: `internal:miss,mirror:hit`.
    pub fn trail_line(&self) -> String {
        trail_text(&self.trail)
    }
}

/// A trail on one line: `internal:miss,mirror:hit`.
pub(super) fn trail_text(trail: &[(String, Step)]) -> String {
    trail
        .iter()
        .map(|(member, step)| format!("{member}:{}", step.as_str()))
        .collect::<Vec<_>>()
        .join(",")
}

/// Leaves in the audit the trail of a resolution: which group, in which
/// version of its configuration, which members were queried and how it ended.
pub(crate) fn record_resolution(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    asset_path: &str,
    trail: &str,
    succeeded: bool,
    correlation: &str,
) {
    let result = if succeeded {
        crate::audit::AuditResult::Allowed
    } else {
        crate::audit::AuditResult::Failed
    };
    crate::handlers::record_audit(
        state,
        context.principal().subject().as_str(),
        context.account().as_str(),
        "resolve",
        &format!(
            "{}@v{}/{asset_path} [{trail}]",
            group.repo_key, group.config.version
        ),
        result,
        correlation,
    );
}

fn the_failure(error: GroupError) -> RawApiError {
    // The members were validated when the group was saved; if they now do not add up
    // (another group was reconfigured, a member no longer exists) the group is
    // misconfigured and is not resolved halfway.
    tracing::warn!(error = %error, "a group repository does not resolve");
    RawApiError::Unavailable
}

/// Whether the principal can read this repository. It is Cedar's decision
/// on its HRN, the same one its direct path applies.
fn can_read(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
) -> Result<bool, RawApiError> {
    state
        .authorize_resource
        .execute_repository(
            context.clone(),
            crate::actions::download_asset(),
            repository,
        )
        .map(|outcome| outcome.allows_access())
        .map_err(|_| RawApiError::Internal)
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
    RawApiError,
> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or(RawApiError::Unavailable)?;
    let account = context.account().as_str();
    let mut known: HashMap<String, Lookup> = HashMap::new();
    let mut found: HashMap<String, RepositoryDescriptor> = HashMap::new();
    let mut pending: Vec<String> = config.members.clone();
    while let Some(key) = pending.pop() {
        if key == group.repo_key || known.contains_key(&key) {
            continue;
        }
        if known.len() >= hodei_catalog::group::MAX_NODES {
            return Err(the_failure(GroupError::TooManyNodes));
        }
        let lookup = match reader.repository(account, &key).await {
            Err(CatalogError::Missing { .. }) => Lookup::Missing,
            Err(CatalogError::Denied { .. }) => Lookup::Hidden,
            Err(_) => return Err(RawApiError::Unavailable),
            Ok(descriptor) => {
                if !can_read(state, context, &descriptor)? {
                    Lookup::Hidden
                } else {
                    let members = if descriptor.mode == RepositoryMode::Group {
                        // An internal group that is not read is a broken group:
                        // it fails closed, it is not treated as empty.
                        match GroupConfig::of(&descriptor.config) {
                            Ok(Some(nested)) => nested.members,
                            _ => return Err(RawApiError::Unavailable),
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

/// Resolves a path in a group.
///
/// The group already passed Cedar's decision and the format and scope filter
/// of `the_repository`; here only its members are walked.
pub(crate) async fn the_group_asset(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    asset_path: &str,
    correlation: &str,
) -> Resolution {
    let mut trail: Vec<(String, Step)> = Vec::new();
    let outcome = resolve(state, context, group, asset_path, correlation, &mut trail).await;
    Resolution { outcome, trail }
}

/// The leaf members of a group, in the order they are queried, already
/// read and filtered by what the principal can read.
pub(super) async fn the_leaves(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
) -> Result<Vec<RepositoryDescriptor>, RawApiError> {
    let config = match GroupConfig::of(&group.config) {
        Ok(Some(config)) => config,
        _ => {
            tracing::warn!(group = %group.repo_key, "a group repository has no readable members");
            return Err(RawApiError::Unavailable);
        }
    };
    let (known, mut found) = the_members(state, context, group, &config).await?;
    let walk = hodei_catalog::walk_group(&group.repo_key, group.format, &config.members, &|key| {
        known.get(key).cloned().unwrap_or(Lookup::Missing)
    })
    .map_err(the_failure)?;
    walk.leaves
        .iter()
        .map(|key| found.remove(key).ok_or(RawApiError::Unavailable))
        .collect()
}

/// What a group lets through from its leaves for **a path**: the leaves
/// that are queried and the proxy members that the group's namespace reservation
/// denied.
///
/// The reservation lives in the group's configuration and is always consulted,
/// before looking at what each member has: a reserved path is not served
/// from any public upstream of the group, neither the first nor the last,
/// and a down upstream does not turn it into a fallback. The hosted members
/// are the internal source and the reservation does not touch them.
///
/// The group's policy, if not understood, does not degrade to "no reservation":
/// it is a misconfigured group and is not resolved halfway.
pub(super) async fn the_governed_leaves(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    asset_path: &str,
) -> Result<
    (
        Vec<RepositoryDescriptor>,
        Vec<String>,
        Option<hodei_catalog::Judgement>,
    ),
    RawApiError,
> {
    let leaves = the_leaves(state, context, group).await?;
    let Some(policy) = the_namespace_policy(&group.config)? else {
        return Ok((leaves, Vec::new(), None));
    };
    let judgement = policy.judge(group.format, asset_path);
    if !judgement.is_reserved() {
        return Ok((leaves, Vec::new(), None));
    }
    let mut consulted = Vec::with_capacity(leaves.len());
    let mut denied = Vec::new();
    for member in leaves {
        if member.mode == RepositoryMode::Proxy {
            denied.push(member.repo_key.clone());
        } else {
            consulted.push(member);
        }
    }
    Ok((consulted, denied, Some(judgement)))
}

/// The explanation of a refusal due to a group-level reservation: the rule
/// that applied and the members that were left out.
pub(super) fn the_group_reservation_explanation(
    group: &RepositoryDescriptor,
    judgement: &hodei_catalog::Judgement,
    denied: &[String],
) -> RawApiError {
    tracing::warn!(
        group = %group.repo_key,
        denied = ?denied,
        "a reserved namespace is not served from the public members of the group"
    );
    RawApiError::ReservedNamespace(the_reservation_explanation(judgement, denied))
}

/// The member is read as it would be read via its direct path: this release only
/// serves the global scope.
pub(super) fn global_only(member: &RepositoryDescriptor) -> Result<(), RawApiError> {
    if member.config.digest_scope == hodei_catalog::DigestScope::Global {
        Ok(())
    } else {
        Err(RawApiError::NotImplemented(
            "this service serves the content of a repository only in the global scope",
        ))
    }
}

async fn resolve(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    asset_path: &str,
    correlation: &str,
    trail: &mut Vec<(String, Step)>,
) -> Result<Won, RawApiError> {
    // A proxy without network and without a copy does not have it within reach: the
    // next member is tried. If none serves it, that is the error.
    let mut offline: Option<RawApiError> = None;
    // The members the group's reservation left out: with them, the
    // final refusal is explained instead of looking like a plain 404.
    let (leaves, denied, judgement) =
        the_governed_leaves(state, context, group, asset_path).await?;
    for key in &denied {
        trail.push((key.clone(), Step::Deny));
    }
    for member in leaves {
        let key = member.repo_key.clone();
        if let Err(error) = global_only(&member) {
            trail.push((key, Step::Error));
            return Err(error);
        }
        let looked = match member.mode {
            RepositoryMode::Proxy => {
                match the_proxy_asset(state, context, &member, asset_path, correlation).await {
                    Ok(ProxyServed {
                        asset,
                        status,
                        provenance,
                    }) => Ok(Some((asset, Some((status, provenance))))),
                    // A 404 from a proxy means the upstream does not have it (or
                    // that it is not its own): it is a MISS, not a failure.
                    Err(RawApiError::NotFound) => Ok(None),
                    Err(other) => Err(other),
                }
            }
            _ => the_current_asset(state, context, &member.repo_key, asset_path)
                .await
                .map(|found| found.map(|asset| (asset, None))),
        };
        match looked {
            Ok(Some((asset, served))) => {
                trail.push((key.clone(), Step::Hit));
                return Ok(Won {
                    member,
                    asset,
                    served,
                });
            }
            Ok(None) => trail.push((key.clone(), Step::Miss)),
            Err(error @ RawApiError::Offline(_)) => {
                trail.push((key.clone(), Step::Miss));
                offline = Some(error);
            }
            Err(error) => {
                trail.push((key.clone(), Step::Error));
                return Err(error);
            }
        }
    }
    if !denied.is_empty()
        && offline.is_none()
        && let Some(judgement) = judgement
    {
        return Err(the_group_reservation_explanation(
            group, &judgement, &denied,
        ));
    }
    Err(offline.unwrap_or(RawApiError::NotFound))
}
