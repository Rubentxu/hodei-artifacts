//! Publication freeze for hosted npm and raw repositories (HSP-322 tramo 2,
//! from HSP-161 "Dependency Freeze").
//!
//! A hosted repository may **declare** a freeze in its `config_settings`:
//!
//! ```json
//! "freeze": { "scope": "all", "reason": "release 4.2 audit", "until": "2026-12-01T00:00:00Z" }
//! ```
//!
//! While the freeze is active the repository serves exactly what it holds:
//! publication is refused **before** a single byte is written.
//!
//! # Rules
//!
//! - No declaration, no gate: the normal path is unchanged (opt-in).
//! - `scope = "all"` refuses every write (new package, new version, dist-tag
//!   or metadata change). `scope = "existing"` refuses any change to what is
//!   already published and still admits NEW names/paths. The scope is
//!   mandatory; there is no implicit one.
//! - `until` (RFC 3339, optional) ends the freeze by itself: from that instant
//!   the repository publishes again. Without it the freeze lasts until the
//!   freeze is released (`DELETE ./freeze` of the repository, see below).
//! - A refusal is `409` with a fixed generic reason (the declared `reason`
//!   stays in the operator's log, never in the response) and a `freeze.refused`
//!   audit event.
//! - Nothing here fails open. A declaration that cannot be read is `503`,
//!   nothing is published and `freeze.unavailable` is audited: UNKNOWN is not
//!   PASS.
//!
//! # Who may freeze
//!
//! Declaring a freeze is a denial-of-publication lever, so it is its own
//! Cedar action, [`crate::actions::manage_repository_freeze`]: `CreateRepository`
//! or `UploadAsset` never imply it. The declaration is made when the repository
//! is created or later through `PUT|DELETE
//! /api/v1/catalog/repositories/{name}/freeze`, always under that action (403
//! audited, uniform 404 across accounts), so a principal that can publish can
//! neither declare nor lift a freeze.
//!
//! # What this is not
//!
//! It is a publication freeze, not the reproducible snapshot of HSP-161
//! (named manifest digest, GC pin, offline export). It covers `PUT` on hosted
//! raw repositories (and `DELETE` of their assets) and hosted npm publication,
//! dist-tags and metadata.
//! Migration import refuses to write into a repository that declares one.

use chrono::{DateTime, Utc};
use hodei_authz::VerifiedContext;
use hodei_catalog::RepositoryDescriptor;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::record_audit;

/// The key under `config_settings` that holds the declaration.
pub const FREEZE_SETTING: &str = "freeze";

/// What the client reads when a freeze refuses a write: the declared reason
/// stays in the operator's log.
pub const REFUSED_REASON: &str = "this repository does not accept publications at this time";

/// The longest reason accepted.
const MAX_REASON_LEN: usize = 200;

/// What the freeze blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Every write.
    All,
    /// Changes to what is already published; new names are admitted.
    Existing,
}

/// The declaration of a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreezeConfig {
    /// What is blocked.
    pub scope: Scope,
    /// Why (operator-facing: log only).
    pub reason: String,
    /// When the freeze ends by itself, if ever.
    pub until: Option<DateTime<Utc>>,
}

impl FreezeConfig {
    /// Reads the declaration. `Ok(None)` is a repository without one.
    ///
    /// A declaration that is present but cannot be read is an `Err`, never
    /// `None`: failing open on a badly written freeze would be a bypass.
    pub fn from_settings(settings: &serde_json::Value) -> Result<Option<Self>, String> {
        let Some(declared) = settings.get(FREEZE_SETTING) else {
            return Ok(None);
        };
        let object = declared
            .as_object()
            .ok_or("the freeze declaration is not an object")?;
        if let Some(unknown) = object
            .keys()
            .find(|key| !matches!(key.as_str(), "scope" | "reason" | "until"))
        {
            return Err(format!("freeze.{unknown} is not a known field"));
        }
        let scope = match object.get("scope").and_then(serde_json::Value::as_str) {
            Some("all") => Scope::All,
            Some("existing") => Scope::Existing,
            Some(_) => return Err("freeze.scope must be `all` or `existing`".to_string()),
            None => return Err("freeze.scope is required".to_string()),
        };
        let reason = object
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|reason| !reason.is_empty() && reason.len() <= MAX_REASON_LEN)
            .ok_or("freeze.reason is required (1 to 200 characters)")?
            .to_string();
        let until = match object.get("until") {
            None => None,
            Some(value) => Some(
                value
                    .as_str()
                    .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
                    .map(|instant| instant.with_timezone(&Utc))
                    .ok_or("freeze.until is not an RFC 3339 instant")?,
            ),
        };
        Ok(Some(Self {
            scope,
            reason,
            until,
        }))
    }

    /// Reads the declaration of a repository.
    pub fn of(repository: &RepositoryDescriptor) -> Result<Option<Self>, String> {
        Self::from_settings(&repository.config.config_settings)
    }

    /// Whether the freeze is in force at `now` (`until` is exclusive).
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.until.is_none_or(|until| now < until)
    }

    /// Whether a write is refused: `target_exists` says if it changes
    /// something already published.
    pub fn blocks(&self, now: DateTime<Utc>, target_exists: bool) -> bool {
        self.is_active(now)
            && match self.scope {
                Scope::All => true,
                Scope::Existing => target_exists,
            }
    }
}

/// Whether the repository declares a freeze at all (readable or not).
pub fn is_declared(repository: &RepositoryDescriptor) -> bool {
    repository
        .config
        .config_settings
        .get(FREEZE_SETTING)
        .is_some()
}

/// Why the gate did not let the write through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The repository is frozen (409).
    Frozen,
    /// The declaration cannot be read: fail closed (503).
    Unavailable,
}

/// Decides whether a write may proceed. `target_exists` is whether it changes
/// an already published package or path. It must run **before** any content is
/// written; an `Err` must be returned to the client as is.
pub fn gate(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
    target: &str,
    target_exists: bool,
    correlation: &str,
) -> Result<(), Refusal> {
    let actor = context.principal().subject().as_str();
    let account = context.account().as_str();
    let resource = format!("{}/{}", repository.repo_key, target);
    let config = match FreezeConfig::of(repository) {
        Ok(None) => return Ok(()),
        Ok(Some(config)) => config,
        Err(error) => {
            tracing::warn!(
                target: "hodei_artifacts::freeze",
                error = %error,
                "the freeze declaration of a repository cannot be read"
            );
            record_audit(
                state,
                actor,
                account,
                "freeze.unavailable",
                &resource,
                AuditResult::Failed,
                correlation,
            );
            return Err(Refusal::Unavailable);
        }
    };
    if !config.blocks(state.identity.now(), target_exists) {
        return Ok(());
    }
    tracing::warn!(
        target: "hodei_artifacts::freeze",
        repository = %repository.repo_key,
        path = %target,
        reason = %config.reason,
        "a publication was refused: the repository is frozen"
    );
    record_audit(
        state,
        actor,
        account,
        "freeze.refused",
        &resource,
        AuditResult::Denied,
        correlation,
    );
    Err(Refusal::Frozen)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;

    fn read(value: serde_json::Value) -> Result<Option<FreezeConfig>, String> {
        FreezeConfig::from_settings(&json!({ "freeze": value }))
    }

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 12, day, 0, 0, 0).unwrap()
    }

    #[test]
    fn no_declaration_is_no_gate() {
        assert_eq!(FreezeConfig::from_settings(&json!({})), Ok(None));
    }

    #[test]
    fn a_declaration_reads() {
        let config =
            read(json!({"scope": "all", "reason": " audit ", "until": "2026-12-05T00:00:00Z"}))
                .unwrap()
                .unwrap();
        assert_eq!(config.scope, Scope::All);
        assert_eq!(config.reason, "audit");
        assert_eq!(config.until, Some(at(5)));
        assert_eq!(
            read(json!({"scope": "existing", "reason": "x"}))
                .unwrap()
                .unwrap()
                .until,
            None
        );
    }

    #[test]
    fn every_unreadable_declaration_is_an_error_never_none() {
        let long = "x".repeat(MAX_REASON_LEN + 1);
        for bad in [
            json!("all"),
            json!(true),
            json!({}),
            json!({"reason": "x"}),
            json!({"scope": "all"}),
            json!({"scope": "none", "reason": "x"}),
            json!({"scope": "all", "reason": ""}),
            json!({"scope": "all", "reason": "  "}),
            json!({"scope": "all", "reason": 7}),
            json!({"scope": "all", "reason": long}),
            json!({"scope": "all", "reason": "x", "until": "tomorrow"}),
            json!({"scope": "all", "reason": "x", "until": 5}),
            json!({"scope": "all", "reason": "x", "surprise": 1}),
        ] {
            assert!(read(bad.clone()).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn the_scope_decides_what_is_blocked() {
        let all = read(json!({"scope": "all", "reason": "x"}))
            .unwrap()
            .unwrap();
        assert!(all.blocks(at(1), false));
        assert!(all.blocks(at(1), true));
        let existing = read(json!({"scope": "existing", "reason": "x"}))
            .unwrap()
            .unwrap();
        assert!(!existing.blocks(at(1), false));
        assert!(existing.blocks(at(1), true));
    }

    #[test]
    fn the_freeze_ends_exactly_at_until() {
        let config = read(json!({"scope": "all", "reason": "x", "until": "2026-12-05T00:00:00Z"}))
            .unwrap()
            .unwrap();
        assert!(config.blocks(at(4), true));
        assert!(!config.blocks(at(5), true), "until is exclusive");
        assert!(!config.blocks(at(6), false));
    }
}
