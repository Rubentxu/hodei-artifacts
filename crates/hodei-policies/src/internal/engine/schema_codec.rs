//! Faithful persistence of a Cedar schema
//!
//! A built Cedar schema (`cedar_policy::Schema`) cannot be serialized back:
//! Cedar offers serialization for the *fragments* (`SchemaFragment`), not for
//! the composed schema. The `Debug` rendering that was used before as a
//! shortcut is not a schema, and rebuilding a schema from it always produces
//! an **empty** schema.
//!
//! This module defines the format in which a schema is persisted and
//! recovered:
//!
//! ```json
//! {
//!   "format": "cedar-schema-fragments-v1",
//!   "fragments": [ { ...Cedar fragment... } ]
//! }
//! ```
//!
//! The envelope is explicit and versioned on purpose:
//!
//! - Recovering a schema can never *fail silently*. Content that is not in
//!   this format produces an explicit error, never an empty schema. An empty
//!   schema would report every policy as invalid and, worse, let every policy
//!   pass as valid.
//! - The format is written into the data, so stale content (for instance the
//!   `Debug` rendering of an earlier version) is detectable instead of being
//!   reinterpreted.
//!
//! The fragments are the ones that were registered, so the round trip is
//! faithful: Cedar serializes and deserializes a fragment without loss.

use cedar_policy::{Schema, SchemaError, SchemaFragment};
use serde::{Deserialize, Serialize};

/// Identifier of the persistence envelope format.
///
/// Changing it obliges changing [`decode`] too: an envelope with another
/// format is rejected instead of being reinterpreted.
pub const SCHEMA_ENVELOPE_FORMAT: &str = "cedar-schema-fragments-v1";

/// Error raised while encoding or decoding a persisted schema.
#[derive(Debug, thiserror::Error)]
pub enum SchemaCodecError {
    /// The fragments could not be serialized.
    #[error("the schema fragments could not be serialized: {0}")]
    Encode(String),

    /// The persisted content is not a schema envelope.
    #[error("the stored schema is not a '{SCHEMA_ENVELOPE_FORMAT}' envelope: {0}")]
    UnsupportedFormat(String),

    /// The envelope is well formed but its fragments are not valid.
    #[error("the stored schema fragments could not be decoded: {0}")]
    Decode(String),

    /// The fragments do not compose a valid schema.
    #[error("the stored schema fragments do not form a valid Cedar schema: {0}")]
    InvalidSchema(String),
}

/// The envelope as it is persisted.
#[derive(Debug, Serialize, Deserialize)]
struct SchemaEnvelope {
    /// The envelope format; must be [`SCHEMA_ENVELOPE_FORMAT`].
    format: String,
    /// The Cedar fragments that compose the schema.
    fragments: Vec<serde_json::Value>,
}

/// Serializes schema fragments into the persisted format.
///
/// # Errors
///
/// Returns an error when Cedar cannot serialize one of the fragments.
pub fn encode(fragments: &[SchemaFragment]) -> Result<String, SchemaCodecError> {
    let mut encoded = Vec::with_capacity(fragments.len());

    for fragment in fragments {
        let value = fragment
            .clone()
            .to_json_value()
            .map_err(|e| SchemaCodecError::Encode(e.to_string()))?;
        encoded.push(value);
    }

    let envelope = SchemaEnvelope {
        format: SCHEMA_ENVELOPE_FORMAT.to_string(),
        fragments: encoded,
    };

    serde_json::to_string(&envelope).map_err(|e| SchemaCodecError::Encode(e.to_string()))
}

/// Rebuilds the schema from its persisted content.
///
/// # Errors
///
/// Returns an error — never an empty schema — when the content is not in the
/// expected format, when its fragments are not valid, or when they do not
/// compose a valid schema.
pub fn decode(content: &str) -> Result<Schema, SchemaCodecError> {
    let envelope: SchemaEnvelope = serde_json::from_str(content)
        .map_err(|e| SchemaCodecError::UnsupportedFormat(e.to_string()))?;

    if envelope.format != SCHEMA_ENVELOPE_FORMAT {
        return Err(SchemaCodecError::UnsupportedFormat(format!(
            "expected format '{}', found '{}'",
            SCHEMA_ENVELOPE_FORMAT, envelope.format
        )));
    }

    let mut fragments = Vec::with_capacity(envelope.fragments.len());
    for value in envelope.fragments {
        let fragment = SchemaFragment::from_json_value(value)
            .map_err(|e: SchemaError| SchemaCodecError::Decode(e.to_string()))?;
        fragments.push(fragment);
    }

    Schema::from_schema_fragments(fragments)
        .map_err(|e| SchemaCodecError::InvalidSchema(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A schema with one entity type and one action, as the IAM schema has.
    const SCHEMA_DSL: &str = r#"
namespace Iam {
    entity User {
        name: String
    };
}
action "CreateUser" appliesTo {
    principal: [Iam::User],
    resource: [Iam::User]
};
"#;

    fn fragment() -> SchemaFragment {
        SchemaFragment::from_cedarschema_str(SCHEMA_DSL)
            .expect("the fixture schema must parse")
            .0
    }

    #[test]
    fn a_schema_survives_the_round_trip() {
        let original = Schema::from_schema_fragments(vec![fragment()]).expect("schema");

        let stored = encode(&[fragment()]).expect("the schema must be encodable");
        let restored = decode(&stored).expect("the stored schema must be decodable");

        let original_entities: Vec<String> =
            original.entity_types().map(|e| e.to_string()).collect();
        let restored_entities: Vec<String> =
            restored.entity_types().map(|e| e.to_string()).collect();

        assert_eq!(
            original_entities, restored_entities,
            "the restored schema must declare the same entity types"
        );

        let original_actions: Vec<String> = original.actions().map(|a| a.to_string()).collect();
        let restored_actions: Vec<String> = restored.actions().map(|a| a.to_string()).collect();

        assert_eq!(
            original_actions, restored_actions,
            "the restored schema must declare the same actions"
        );
    }

    #[test]
    fn a_stored_schema_is_not_an_empty_schema() {
        let stored = encode(&[fragment()]).expect("the schema must be encodable");
        let restored = decode(&stored).expect("the stored schema must be decodable");

        assert_eq!(
            restored.entity_types().count(),
            1,
            "a stored schema must come back with its entity types, not empty"
        );
    }

    #[test]
    fn a_content_that_is_not_an_envelope_is_rejected() {
        // The representation that was used before the envelope existed: the
        // `Debug` rendering of a schema. It must be rejected explicitly, not
        // reinterpreted as an empty schema.
        let result = decode("ValidatorSchema { entity_types: {}, action_ids: {} }");

        assert!(
            result.is_err(),
            "content that is not an envelope must be rejected, not silently read as an empty schema"
        );
    }

    #[test]
    fn an_envelope_with_an_unknown_format_is_rejected() {
        let stored = serde_json::json!({
            "format": "some-other-format",
            "fragments": [],
        })
        .to_string();

        let result = decode(&stored);

        match result {
            Err(SchemaCodecError::UnsupportedFormat(_)) => {}
            other => panic!("expected UnsupportedFormat, got: {other:?}"),
        }
    }

    #[test]
    fn an_empty_envelope_decodes_to_an_empty_schema() {
        // An envelope with no fragments is a valid, explicitly empty schema —
        // as opposed to a content that is not an envelope at all.
        let stored = encode(&[]).expect("an empty schema must be encodable");
        let restored = decode(&stored).expect("an empty envelope must be decodable");

        assert_eq!(restored.entity_types().count(), 0);
    }
}
