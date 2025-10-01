//! Actions schema utilities
//!
//! Provides a Cedar schema fragment that declares one action per feature
//! directory in `crates/policies/src/features/`.

use cedar_policy::{CedarSchemaError, SchemaFragment};

/// List of actions (derived from feature directory names)
///
/// Directories observed:
/// - audit_policy
/// - create_policy
/// - delete_policy
/// - evaluate_policy
/// - get_policy
/// - hierarchical_policy
/// - list_policies
/// - manage_policy_versions
/// - policy_playground
/// - update_policy
/// - validate_policy
const FEATURE_ACTIONS: &[&str] = &[
    "audit_policy",
    "create_policy",
    "delete_policy",
    "evaluate_policy",
    "get_policy",
    "hierarchical_policy",
    "list_policies",
    "manage_policy_versions",
    "policy_playground",
    "update_policy",
    "validate_policy",
];


/// Build a Cedar schema fragment that declares one action per feature name.
///
/// The actions are declared with appliesTo matching our domain:
/// principal `User` y resource `Resource`.
pub fn build_feature_actions_fragment() -> Result<SchemaFragment, CedarSchemaError> {
    let mut s = String::new();
    // Ensure the base types exist where this fragment is used; callers should
    // include appropriate entity declarations in their overall schema.
    for act in FEATURE_ACTIONS {
        // Convert to a Cedar-friendly identifier (letters, digits, '_')
        let ced = normalize_action_ident(act);
        s.push_str(&format!(
            "action {} appliesTo {{ principal: User, resource: Resource }};\n",
            ced
        ));
    }
    // No extra actions here: tests can use actions without schema appliesTo
    SchemaFragment::from_cedarschema_str(&s).map(|(frag, _)| frag)
}

fn normalize_action_ident(name: &str) -> String {
    let mut out = String::new();
    let mut chars = name.chars();
    if let Some(c0) = chars.next() {
        let c = if c0.is_ascii_alphabetic() || c0 == '_' {
            c0
        } else {
            '_'
        };
        out.push(c);
    } else {
        out.push('_');
    }
    for c in chars {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_fragment_parses() {
        let frag = build_feature_actions_fragment();
        assert!(
            frag.is_ok(),
            "actions fragment should parse: {:?}",
            frag.err()
        );
    }
}
