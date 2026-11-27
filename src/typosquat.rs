//! Publish-time look-alike (typosquatting) gate for hosted npm repositories
//! (HSP-322 tramo 1, from HSP-166).
//!
//! A hosted npm repository may **declare** a naming policy in its
//! `config_settings`:
//!
//! ```json
//! "typosquat": { "mode": "block", "protected": ["react", "lodash"], "scan_registry": true }
//! ```
//!
//! When a publish would create a NEW package name, [`gate`] compares it with
//! the names the repository already holds (the packuments of the registry
//! itself, unless `scan_registry` is `false`) plus the administrator's
//! optional `protected` list, through the pure judgement of
//! [`hodei_risk::advisories::find_lookalike`] (skeleton of confusables and
//! edit distance 1). It runs **before** a single byte is written.
//!
//! # Rules
//!
//! - No declaration, no gate: the normal path is unchanged (opt-in).
//! - Only a NEW name is judged. A new version of a package that already exists
//!   is the package itself, not a squat of it.
//! - `mode = "block"` refuses the publication (`403`, generic reason: neither
//!   the matched name nor the rule leave the service) and audits
//!   `typosquat.refused`. `mode = "audit"` publishes and audits
//!   `typosquat.flagged`: a rollout step, never the default (the mode is
//!   mandatory, there is no implicit one).
//! - Nothing here fails open. A declaration that cannot be read, or a registry
//!   listing that cannot be completed (store down, more assets than the
//!   catalog will list), is `503` and nothing is published: UNKNOWN is not
//!   PASS.
//!
//! # What this is not
//!
//! It is a heuristic about names, not a malware verdict. It is self-declared
//! by whoever creates the repository (like the namespace policy, it needs no
//! dedicated action: it restricts, never widens, and it has no outbound
//! side). It is scoped to the repository's own registry: it does not see
//! other repositories of the account, which a principal may not be able to
//! read. A name equal to a `protected` one is not a squat of it; reserving a
//! name is the namespace policy's job (HSP-143).

use hodei_authz::VerifiedContext;
use hodei_catalog::RepositoryDescriptor;
use hodei_catalog::npm::PACKUMENT_COORDINATES_SUFFIX;
use hodei_risk::advisories::{Ecosystem, find_lookalike};

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::record_audit;
use crate::resolution::the_catalog;

/// The key under `config_settings` that holds the declaration.
pub const TYPOSQUAT_SETTING: &str = "typosquat";

/// The most names an administrator may protect explicitly.
const MAX_PROTECTED: usize = 1_000;
/// The longest protected name accepted (npm's own limit is 214).
const MAX_NAME_LEN: usize = 214;

/// What the repository does with a look-alike name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Refuse the publication.
    Block,
    /// Publish and leave an audit event.
    Audit,
}

/// The declaration of a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TyposquatConfig {
    /// Refuse or only audit.
    pub mode: Mode,
    /// Names the administrator protects in addition to the registry's own.
    pub protected: Vec<String>,
    /// Whether the names already in the repository are compared too.
    pub scan_registry: bool,
}

impl TyposquatConfig {
    /// Reads the declaration. `Ok(None)` is a repository without one.
    ///
    /// A declaration that is present but cannot be read is an `Err`, never
    /// `None`: failing open on a badly written policy would be a bypass.
    pub fn from_settings(settings: &serde_json::Value) -> Result<Option<Self>, String> {
        let Some(declared) = settings.get(TYPOSQUAT_SETTING) else {
            return Ok(None);
        };
        let object = declared
            .as_object()
            .ok_or("the typosquat declaration is not an object")?;
        if let Some(unknown) = object
            .keys()
            .find(|key| !matches!(key.as_str(), "mode" | "protected" | "scan_registry"))
        {
            return Err(format!("typosquat.{unknown} is not a known field"));
        }
        let mode = match object.get("mode").and_then(serde_json::Value::as_str) {
            Some("block") => Mode::Block,
            Some("audit") => Mode::Audit,
            Some(_) => return Err("typosquat.mode must be `block` or `audit`".to_string()),
            None => return Err("typosquat.mode is required".to_string()),
        };
        let protected = match object.get("protected") {
            None => Vec::new(),
            Some(list) => {
                let list = list.as_array().ok_or("typosquat.protected is not a list")?;
                if list.len() > MAX_PROTECTED {
                    return Err(format!(
                        "typosquat.protected holds more than {MAX_PROTECTED} names"
                    ));
                }
                list.iter()
                    .map(|item| {
                        item.as_str()
                            .filter(|name| !name.is_empty() && name.len() <= MAX_NAME_LEN)
                            .map(str::to_string)
                            .ok_or("typosquat.protected holds an empty, long or non-string name")
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        let scan_registry = match object.get("scan_registry") {
            None => true,
            Some(value) => value
                .as_bool()
                .ok_or("typosquat.scan_registry is not a boolean")?,
        };
        if !scan_registry && protected.is_empty() {
            // Nothing to compare with would admit everything under a policy
            // that looks active: a misdeclaration never opens a false gate.
            return Err(
                "typosquat compares with nothing: scan_registry is false and protected is empty"
                    .to_string(),
            );
        }
        Ok(Some(Self {
            mode,
            protected,
            scan_registry,
        }))
    }

    /// Reads the declaration of a repository.
    pub fn of(repository: &RepositoryDescriptor) -> Result<Option<Self>, String> {
        Self::from_settings(&repository.config.config_settings)
    }
}

/// Whether the repository declares the policy at all (readable or not).
pub fn is_declared(repository: &RepositoryDescriptor) -> bool {
    repository
        .config
        .config_settings
        .get(TYPOSQUAT_SETTING)
        .is_some()
}

/// Why the gate did not let the publication through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The name looks like an existing one and the mode is `block` (403).
    Rejected,
    /// The gate could not decide: fail closed (503).
    Unavailable,
}

/// What the pure decision says about a name, given the names to compare with.
///
/// Separated from the I/O so the policy semantics are testable without a
/// store: `Ok(Some(why))` is a look-alike, `Ok(None)` is clean.
pub fn judge<'a>(
    config: &'a TyposquatConfig,
    candidate: &str,
    registry_names: impl IntoIterator<Item = &'a str>,
) -> Option<hodei_risk::advisories::Lookalike> {
    let protected = config.protected.iter().map(String::as_str);
    find_lookalike(Ecosystem::Npm, candidate, protected.chain(registry_names))
}

/// The names of the packages the repository already holds.
async fn registry_names(
    state: &AppState,
    account: &str,
    repository: &str,
) -> Result<Vec<String>, Refusal> {
    let (reader, _) = the_catalog(state).map_err(|_| Refusal::Unavailable)?;
    // `assets_under` fails rather than returning a truncated list: a partial
    // view of the registry would look complete and let a squat through.
    let assets = reader
        .assets_under(account, repository, "")
        .await
        .map_err(|error| {
            tracing::warn!(
                target: "hodei_artifacts::typosquat",
                error = %error,
                "the registry names could not be listed"
            );
            Refusal::Unavailable
        })?;
    Ok(assets
        .into_iter()
        .filter_map(|asset| {
            asset
                .coordinates
                .package_name
                .strip_suffix(PACKUMENT_COORDINATES_SUFFIX)
                .map(str::to_string)
        })
        .collect())
}

/// Decides whether a NEW npm package name may be published into `repository`.
///
/// The caller invokes it only when the name does not exist yet. `Ok(())` means
/// publish; an `Err` must be returned **before** any content is written.
pub async fn gate(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    name: &str,
    correlation: &str,
) -> Result<(), Refusal> {
    let actor = context.principal().subject().as_str();
    let account = context.account().as_str();
    let resource = format!("{}/{}", repository.repo_key, name);
    let config = match TyposquatConfig::of(repository) {
        Ok(None) => return Ok(()),
        Ok(Some(config)) => config,
        Err(error) => {
            tracing::warn!(
                target: "hodei_artifacts::typosquat",
                error = %error,
                "the typosquat declaration of a repository cannot be read"
            );
            record_audit(
                state,
                actor,
                account,
                "typosquat.unavailable",
                &resource,
                AuditResult::Failed,
                correlation,
            );
            return Err(Refusal::Unavailable);
        }
    };
    let existing = if config.scan_registry {
        match registry_names(state, account, &repository.repo_key).await {
            Ok(names) => names,
            Err(refusal) => {
                record_audit(
                    state,
                    actor,
                    account,
                    "typosquat.unavailable",
                    &resource,
                    AuditResult::Failed,
                    correlation,
                );
                return Err(refusal);
            }
        }
    } else {
        Vec::new()
    };
    let Some(lookalike) = judge(&config, name, existing.iter().map(String::as_str)) else {
        return Ok(());
    };
    // The matched name and the rule go to the operator's log, never to the client.
    tracing::warn!(
        target: "hodei_artifacts::typosquat",
        repository = %repository.repo_key,
        name = %name,
        looks_like = %lookalike.known,
        rule = lookalike.kind.as_str(),
        "the name looks like an existing package"
    );
    match config.mode {
        Mode::Block => {
            record_audit(
                state,
                actor,
                account,
                "typosquat.refused",
                &resource,
                AuditResult::Denied,
                correlation,
            );
            Err(Refusal::Rejected)
        }
        Mode::Audit => {
            record_audit(
                state,
                actor,
                account,
                "typosquat.flagged",
                &resource,
                AuditResult::Allowed,
                correlation,
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn read(value: serde_json::Value) -> Result<Option<TyposquatConfig>, String> {
        TyposquatConfig::from_settings(&json!({ "typosquat": value }))
    }

    #[test]
    fn no_declaration_is_no_gate() {
        assert_eq!(TyposquatConfig::from_settings(&json!({})), Ok(None));
    }

    #[test]
    fn a_declaration_reads_with_the_registry_scan_on_by_default() {
        let config = read(json!({ "mode": "block", "protected": ["react"] }))
            .unwrap()
            .unwrap();
        assert_eq!(config.mode, Mode::Block);
        assert!(config.scan_registry);
        assert_eq!(config.protected, vec!["react".to_string()]);
    }

    #[test]
    fn every_unreadable_declaration_is_an_error_never_none() {
        for bad in [
            json!("block"),
            json!({}),
            json!({ "mode": "off" }),
            json!({ "mode": "block", "protected": "react" }),
            json!({ "mode": "block", "protected": [""] }),
            json!({ "mode": "block", "protected": [1] }),
            json!({ "mode": "block", "scan_registry": "yes" }),
            json!({ "mode": "block", "scan_registry": false }),
            json!({ "mode": "block", "surprise": true }),
        ] {
            assert!(read(bad.clone()).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn the_protected_list_is_bounded() {
        let many: Vec<String> = (0..=MAX_PROTECTED).map(|n| format!("pkg-{n}")).collect();
        assert!(read(json!({ "mode": "block", "protected": many })).is_err());
    }

    #[test]
    fn a_protected_name_catches_a_squat_but_not_itself() {
        let config = read(json!({ "mode": "block", "protected": ["react"] }))
            .unwrap()
            .unwrap();
        assert!(judge(&config, "raect", []).is_some());
        assert!(judge(&config, "react", []).is_none());
        assert!(judge(&config, "left-pad", []).is_none());
    }

    #[test]
    fn the_registry_names_join_the_protected_ones() {
        let config = read(json!({ "mode": "block", "protected": ["react"] }))
            .unwrap()
            .unwrap();
        assert!(judge(&config, "acme-utils", ["acme-utlis"]).is_some());
    }
}
