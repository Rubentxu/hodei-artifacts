//! The authority of a schema over a request
//!
//! A Cedar schema is not only the set of types a policy is checked against.
//! It is also the **authority** over the shape of the request that may be
//! evaluated: which verb is being asked for, in which namespace, and whether
//! the request conforms at all.
//!
//! This module holds the two questions the engine asks a schema before it
//! decides anything:
//!
//! 1. **Does the schema say anything at all?** A schema that declares neither
//!    an entity type nor an action constrains nothing. Applying it as it is
//!    would reject requests it knows nothing about — a *false rejection*,
//!    not a validation.
//! 2. **Which action does the requested verb denote?** The action's
//!    namespace is not a formatting choice: it is what the schema declares.
//!    Cedar allows two *namespaced* actions to share an id, so the answer can
//!    be ambiguous, and an ambiguous answer must never be resolved by
//!    picking one of the candidates.

use cedar_policy::{EntityUid, Schema};

/// Whether the schema declares something a request can be checked against.
///
/// A schema with no entity type and no action says nothing about the world,
/// so it cannot tell a conformant request from a non-conformant one.
pub fn declares_anything(schema: &Schema) -> bool {
    schema.entity_types().count() > 0 || schema.actions().count() > 0
}

/// How the schema resolves the requested action.
#[derive(Debug, Clone)]
pub enum ActionResolution {
    /// Exactly one action of the schema carries the requested id.
    Resolved(EntityUid),
    /// More than one action carries the requested id.
    ///
    /// Cedar allows it: two *namespaced* actions may share an id. Picking one
    /// of them would evaluate the request against an action the caller did
    /// not ask for, so the ambiguity is reported instead.
    Ambiguous(Vec<EntityUid>),
    /// No action of the schema carries the requested id.
    NotDeclared,
}

/// Resolve the requested action against the schema.
///
/// The action is looked up **by id**: the caller names the verb and the
/// schema decides which namespace it belongs to. The lookup is exact — an
/// action whose id differs is a different action, not a near match.
pub fn resolve_action(schema: &Schema, action_id: &str) -> ActionResolution {
    let candidates: Vec<EntityUid> = schema
        .actions()
        .filter(|uid| uid.id().unescaped() == action_id)
        .cloned()
        .collect();

    match candidates.as_slice() {
        [] => ActionResolution::NotDeclared,
        [uid] => ActionResolution::Resolved(uid.clone()),
        _ => ActionResolution::Ambiguous(candidates),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cedar_policy::SchemaFragment;

    /// A schema with the entity type `Iam::User` and no action at all.
    fn schema_declaring_user() -> Schema {
        let fragment = SchemaFragment::from_cedarschema_str(
            r#"
namespace Iam {
    entity User {
        name: String
    };
}
"#,
        )
        .expect("the fixture schema must parse")
        .0;

        Schema::from_schema_fragments(vec![fragment]).expect("the fixture schema must compose")
    }

    /// A schema with the entity type `Iam::User` and the flat action `read`.
    fn schema_declaring_flat_action() -> Schema {
        let fragment = SchemaFragment::from_cedarschema_str(
            r#"
namespace Iam {
    entity User {
        name: String
    };
}
action "read" appliesTo { principal: [Iam::User], resource: [Iam::User] };
"#,
        )
        .expect("the fixture schema must parse")
        .0;

        Schema::from_schema_fragments(vec![fragment]).expect("the fixture schema must compose")
    }

    /// A schema with the entity type `Iam::User` and the namespaced action
    /// `Hodei::Action::"read"`.
    fn schema_declaring_namespaced_action() -> Schema {
        let fragment = SchemaFragment::from_cedarschema_str(
            r#"
namespace Iam {
    entity User {
        name: String
    };
}
namespace Hodei {
    action "read" appliesTo { principal: [Iam::User], resource: [Iam::User] };
}
"#,
        )
        .expect("the fixture schema must parse")
        .0;

        Schema::from_schema_fragments(vec![fragment]).expect("the fixture schema must compose")
    }

    /// A schema with two namespaced actions that share the id `read`.
    fn schema_declaring_ambiguous_action() -> Schema {
        let fragment = SchemaFragment::from_cedarschema_str(
            r#"
namespace Iam {
    entity User {
        name: String
    };
}
namespace Hodei {
    action "read" appliesTo { principal: [Iam::User], resource: [Iam::User] };
}
namespace Other {
    action "read" appliesTo { principal: [Iam::User], resource: [Iam::User] };
}
"#,
        )
        .expect("the fixture schema must parse")
        .0;

        Schema::from_schema_fragments(vec![fragment]).expect("the fixture schema must compose")
    }

    #[test]
    fn test_an_empty_schema_declares_nothing() {
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        assert!(!declares_anything(&schema));
    }

    #[test]
    fn test_a_schema_with_only_an_entity_type_declares_something() {
        assert!(declares_anything(&schema_declaring_user()));
    }

    #[test]
    fn test_a_schema_with_only_an_action_declares_something() {
        assert!(declares_anything(&schema_declaring_flat_action()));
    }

    #[test]
    fn test_a_flat_action_is_resolved_as_declared() {
        let schema = schema_declaring_flat_action();

        let resolution = resolve_action(&schema, "read");

        assert!(
            matches!(&resolution, ActionResolution::Resolved(uid) if uid.to_string() == "Action::\"read\""),
            "the flat action must resolve to the form the schema declares: {resolution:?}"
        );
    }

    #[test]
    fn test_a_namespaced_action_is_resolved_as_declared() {
        let schema = schema_declaring_namespaced_action();

        let resolution = resolve_action(&schema, "read");

        assert!(
            matches!(&resolution, ActionResolution::Resolved(uid) if uid.to_string() == "Hodei::Action::\"read\""),
            "the schema decides the namespace of the action: {resolution:?}"
        );
    }

    #[test]
    fn test_an_undeclared_action_is_not_declared() {
        let schema = schema_declaring_flat_action();

        let resolution = resolve_action(&schema, "write");

        assert!(
            matches!(resolution, ActionResolution::NotDeclared),
            "an action the schema does not declare must not be resolved: {resolution:?}"
        );
    }

    #[test]
    fn test_two_actions_sharing_an_id_are_ambiguous() {
        let schema = schema_declaring_ambiguous_action();

        let resolution = resolve_action(&schema, "read");

        match resolution {
            ActionResolution::Ambiguous(candidates) => {
                assert_eq!(candidates.len(), 2, "both candidates must be reported");
                assert!(
                    candidates.iter().all(|uid| uid.id().unescaped() == "read"),
                    "both candidates must carry the requested id"
                );
            }
            other => panic!("two actions sharing an id must be ambiguous: {other:?}"),
        }
    }
}
