//! The replacement policy for the content of an address.
//!
//! A **hosted** raw repository stores content at an address.
//! Publishing to an address that already has content **replaces** what
//! is there, and replacing without further ado is a decision the
//! client cannot make: two clients publishing to the same address overwrite each other,
//! and the one that loses does not find out.
//!
//! That is why the decision is not the client's: it is the **repository's**, and lives
//! in its **versioned** configuration. By default an address publishes
//! **once**: publishing the same content again is idempotent, and
//! publishing other content is a conflict.
//!
//! # Where the decision lives
//!
//! In the repository settings, under the key `raw.replace`:
//!
//! ```json
//! { "raw": { "replace": "allow" } }
//! ```
//!
//! `allow` permits replacement. `deny` --or the absence of the key-- does not. The absence
//! is a decision: a repository that says nothing is a repository that
//! has not said replacement is allowed, and what has not been said is not
//! permitted.

use serde_json::Value;

/// Whether the content of an address can be replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacePolicy {
    /// No: the address publishes once.
    Deny,
    /// Yes: the address can be replaced.
    Allow,
}

/// The policy key within the repository settings.
const REPLACE_KEY: &str = "replace";

/// The value that authorizes replacement.
const ALLOW: &str = "allow";

/// The replacement policy stated by the repository configuration.
///
/// What is not `allow` is `deny`: an unknown value is not a permission,
/// and a permission that is not named is not granted.
pub fn the_replace_policy(settings: &Value) -> ReplacePolicy {
    let allowed = settings
        .get("raw")
        .and_then(|raw| raw.get(REPLACE_KEY))
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case(ALLOW));
    if allowed {
        ReplacePolicy::Allow
    } else {
        ReplacePolicy::Deny
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_absence_of_the_policy_is_a_refusal() {
        assert_eq!(the_replace_policy(&json!({})), ReplacePolicy::Deny);
        assert_eq!(
            the_replace_policy(&json!({ "raw": {} })),
            ReplacePolicy::Deny
        );
    }

    #[test]
    fn only_allow_replaces() {
        assert_eq!(
            the_replace_policy(&json!({ "raw": { "replace": "allow" } })),
            ReplacePolicy::Allow
        );
        assert_eq!(
            the_replace_policy(&json!({ "raw": { "replace": "deny" } })),
            ReplacePolicy::Deny
        );
    }

    #[test]
    fn an_unknown_value_is_not_a_permission() {
        assert_eq!(
            the_replace_policy(&json!({ "raw": { "replace": "true" } })),
            ReplacePolicy::Deny
        );
        assert_eq!(
            the_replace_policy(&json!({ "raw": { "replace": 1 } })),
            ReplacePolicy::Deny
        );
        assert_eq!(
            the_replace_policy(&json!({ "raw": "allow" })),
            ReplacePolicy::Deny
        );
    }
}
