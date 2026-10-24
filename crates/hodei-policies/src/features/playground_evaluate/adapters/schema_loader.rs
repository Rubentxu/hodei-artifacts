//! Schema Loader Adapter for Playground Evaluate
//!
//! This adapter implements the SchemaLoaderPort trait by integrating with
//! the load_schema feature to load Cedar schemas from storage or parse
//! inline schemas.

use super::super::error::PlaygroundEvaluateError;
use super::super::ports::SchemaLoaderPort;
use crate::features::load_schema::ports::SchemaStoragePort as LoadSchemaStoragePort;
use async_trait::async_trait;
use cedar_policy::Schema;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Adapter that implements SchemaLoaderPort using the load_schema feature
///
/// This adapter provides two modes of operation:
/// 1. Inline schema parsing: Parse Cedar schema from JSON string
/// 2. Storage loading: Load schema from storage using load_schema feature
///
/// # Architecture
///
/// This adapter bridges the playground_evaluate feature with the load_schema
/// feature, enabling schema reuse without duplicating logic.
pub struct SchemaLoaderAdapter {
    /// Storage port from load_schema feature (optional)
    /// If None, only inline schemas are supported
    storage: Option<Arc<dyn LoadSchemaStoragePort>>,
}

impl SchemaLoaderAdapter {
    /// Create a new schema loader adapter with storage support
    ///
    /// # Arguments
    ///
    /// * `storage` - Storage implementation for loading schemas from persistence
    pub fn new(storage: Arc<dyn LoadSchemaStoragePort>) -> Self {
        Self {
            storage: Some(storage),
        }
    }

    /// Create a new schema loader adapter without storage (inline-only)
    ///
    /// This is useful for testing or environments where only inline schemas
    /// are needed.
    pub fn new_inline_only() -> Self {
        Self { storage: None }
    }

    /// Parse an inline Cedar schema from JSON string
    ///
    /// The inline schema is read with the same codec the persistence path
    /// uses (`cedar-schema-fragments-v1`), so a schema that arrives inline is
    /// read exactly like one that was stored. A content that is not in that
    /// format is rejected explicitly: discarding it and returning an empty
    /// schema would evaluate the policies against *no* schema, which reports
    /// every policy that mentions a declared type as invalid and every
    /// policy that mentions nothing as valid.
    ///
    /// An empty JSON object and `null` are the one exception: they are an
    /// explicit request for an empty schema.
    ///
    /// # Arguments
    ///
    /// * `schema_json` - Cedar schema envelope in JSON format
    ///
    /// # Returns
    ///
    /// A parsed Cedar Schema
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Schema JSON is invalid
    /// - The content is not a readable schema envelope
    fn parse_inline_schema(&self, schema_json: &str) -> Result<Schema, PlaygroundEvaluateError> {
        debug!("Parsing inline schema");

        // Parse JSON to validate it first
        let json_value: serde_json::Value = serde_json::from_str(schema_json).map_err(|e| {
            PlaygroundEvaluateError::InvalidInlineSchema(format!("Invalid JSON: {}", e))
        })?;

        // An explicitly empty request is an empty schema, not a failure.
        if json_value.is_null()
            || (json_value.is_object() && json_value.as_object().unwrap().is_empty())
        {
            info!("Creating empty schema from inline JSON");
            return Schema::from_schema_fragments(vec![]).map_err(|e| {
                PlaygroundEvaluateError::SchemaError(format!(
                    "Failed to create empty schema: {}",
                    e
                ))
            });
        }

        // The inline schema is read with the codec the persistence path uses,
        // so both read paths agree on what a schema is.
        let schema = crate::internal::engine::schema_codec::decode(schema_json).map_err(|e| {
            warn!(error = %e, "The inline schema is not a readable schema envelope");
            PlaygroundEvaluateError::InvalidInlineSchema(format!(
                "the inline schema is not a '{}' envelope: {}",
                crate::internal::engine::schema_codec::SCHEMA_ENVELOPE_FORMAT,
                e
            ))
        })?;

        info!("Decoded the inline schema");
        Ok(schema)
    }

    /// Load a schema from storage using the provided version
    ///
    /// # Arguments
    ///
    /// * `version` - Schema version identifier
    ///
    /// # Returns
    ///
    /// A loaded Cedar Schema
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Storage is not configured
    /// - Schema version not found
    /// - Schema loading fails
    async fn load_from_storage(&self, version: &str) -> Result<Schema, PlaygroundEvaluateError> {
        debug!(version = %version, "Loading schema from storage");

        let storage = self.storage.as_ref().ok_or_else(|| {
            PlaygroundEvaluateError::SchemaStorageError(
                "Storage not configured for schema loading".to_string(),
            )
        })?;

        // Load schema using load_schema feature
        let stored_schema = storage
            .load_schema(Some(version.to_string()))
            .await
            .map_err(|e| {
                warn!(version = %version, error = %e, "Failed to load schema from storage");
                PlaygroundEvaluateError::SchemaNotFound(version.to_string())
            })?;

        info!(version = %version, "Successfully loaded schema from storage");

        // Parse the stored schema
        stored_schema.parse().map_err(|e| {
            PlaygroundEvaluateError::SchemaError(format!("Failed to parse stored schema: {}", e))
        })
    }
}

#[async_trait]
impl SchemaLoaderPort for SchemaLoaderAdapter {
    async fn load_schema(
        &self,
        inline_schema: Option<String>,
        schema_version: Option<String>,
    ) -> Result<Schema, PlaygroundEvaluateError> {
        match (inline_schema, schema_version) {
            (Some(inline), None) => {
                // Load inline schema
                self.parse_inline_schema(&inline)
            }
            (None, Some(version)) => {
                // Load from storage
                self.load_from_storage(&version).await
            }
            (Some(_), Some(_)) => {
                // Both provided - error
                Err(PlaygroundEvaluateError::InvalidCommand(
                    "Cannot provide both inline_schema and schema_version".to_string(),
                ))
            }
            (None, None) => {
                // Neither provided - error
                Err(PlaygroundEvaluateError::InvalidCommand(
                    "Must provide either inline_schema or schema_version".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::build_schema::ports::StoredSchema;
    use crate::features::load_schema::ports::SchemaStoragePort as LoadSchemaStoragePort;

    /// A schema envelope that declares one entity type, as the IAM schema does.
    ///
    /// It is written with the codec the persistence path uses, so it is
    /// exactly what a stored schema looks like.
    fn envelope_declaring_user() -> String {
        let fragment = cedar_policy::SchemaFragment::from_cedarschema_str(
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

        crate::internal::engine::schema_codec::encode(&[fragment])
            .expect("the fixture schema must be encodable")
    }

    struct MockStorage {
        schema: Option<StoredSchema>,
    }

    #[async_trait]
    impl LoadSchemaStoragePort for MockStorage {
        async fn load_schema(
            &self,
            _version: Option<String>,
        ) -> Result<StoredSchema, crate::features::build_schema::error::BuildSchemaError> {
            self.schema.clone().ok_or_else(|| {
                crate::features::build_schema::error::BuildSchemaError::SchemaStorageError(
                    "Schema not found".to_string(),
                )
            })
        }

        async fn save_schema(
            &self,
            _schema_json: String,
            _version: Option<String>,
        ) -> Result<String, crate::features::build_schema::error::BuildSchemaError> {
            Ok("mock-id".to_string())
        }

        async fn get_latest_schema(
            &self,
        ) -> Result<Option<String>, crate::features::build_schema::error::BuildSchemaError>
        {
            Ok(None)
        }

        async fn get_schema_by_version(
            &self,
            _version: &str,
        ) -> Result<Option<String>, crate::features::build_schema::error::BuildSchemaError>
        {
            Ok(None)
        }

        async fn delete_schema(
            &self,
            _schema_id: &str,
        ) -> Result<bool, crate::features::build_schema::error::BuildSchemaError> {
            Ok(false)
        }

        async fn list_schema_versions(
            &self,
        ) -> Result<Vec<String>, crate::features::build_schema::error::BuildSchemaError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_parse_empty_inline_schema() {
        let adapter = SchemaLoaderAdapter::new_inline_only();
        let result = adapter.load_schema(Some("{}".to_string()), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_schema_requires_one_parameter() {
        let adapter = SchemaLoaderAdapter::new_inline_only();
        let result = adapter.load_schema(None, None).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PlaygroundEvaluateError::InvalidCommand(_)
        ));
    }

    #[tokio::test]
    async fn test_load_schema_rejects_both_parameters() {
        let adapter = SchemaLoaderAdapter::new_inline_only();
        let result = adapter
            .load_schema(Some("{}".to_string()), Some("v1".to_string()))
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PlaygroundEvaluateError::InvalidCommand(_)
        ));
    }

    #[tokio::test]
    async fn test_load_from_storage_without_storage_configured() {
        let adapter = SchemaLoaderAdapter::new_inline_only();
        let result = adapter.load_schema(None, Some("v1".to_string())).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PlaygroundEvaluateError::SchemaStorageError(_)
        ));
    }

    #[tokio::test]
    async fn test_load_from_storage_schema_not_found() {
        let storage = Arc::new(MockStorage { schema: None });
        let adapter = SchemaLoaderAdapter::new(storage);
        let result = adapter.load_schema(None, Some("v1".to_string())).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PlaygroundEvaluateError::SchemaNotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_parse_invalid_json() {
        let adapter = SchemaLoaderAdapter::new_inline_only();
        let result = adapter
            .load_schema(Some("invalid json".to_string()), None)
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PlaygroundEvaluateError::InvalidInlineSchema(_)
        ));
    }

    /// An inline envelope is read as the schema it declares
    ///
    /// The defect this protects against is silent: the loader used to accept
    /// any non-empty JSON, warn about it and continue with an *empty* schema.
    /// An empty schema declares nothing, so the policies were checked against
    /// no schema at all and every one of them looked valid.
    #[tokio::test]
    async fn test_inline_envelope_is_read_as_the_schema_it_declares() {
        let adapter = SchemaLoaderAdapter::new_inline_only();

        let schema = adapter
            .load_schema(Some(envelope_declaring_user()), None)
            .await
            .expect("an envelope must be readable");

        let declared: Vec<String> = schema.entity_types().map(|e| e.to_string()).collect();

        assert_eq!(
            declared,
            vec!["Iam::User".to_string()],
            "the inline schema must declare its entity types, not come back empty"
        );
    }

    /// A content that is not an envelope is rejected, not silently emptied
    ///
    /// This is the discriminating counterpart of the previous test: the
    /// content below is a Cedar schema in the human-readable format, so it
    /// looks like a schema. It is not the envelope the read paths share, so
    /// it is rejected instead of being read as an empty schema.
    #[tokio::test]
    async fn test_inline_content_that_is_not_an_envelope_is_rejected() {
        let adapter = SchemaLoaderAdapter::new_inline_only();

        let result = adapter
            .load_schema(
                Some(
                    r#"{"entityTypes": {"User": {"shape": {"type": "Record", "attributes": {}}}}}"#
                        .to_string(),
                ),
                None,
            )
            .await;

        assert!(
            result.is_err(),
            "a content that is not an envelope must be rejected, not read as an empty schema"
        );
        assert!(matches!(
            result.unwrap_err(),
            PlaygroundEvaluateError::InvalidInlineSchema(_)
        ));
    }

    /// An envelope with an unknown format is rejected
    #[tokio::test]
    async fn test_inline_envelope_with_an_unknown_format_is_rejected() {
        let adapter = SchemaLoaderAdapter::new_inline_only();

        let result = adapter
            .load_schema(
                Some(
                    serde_json::json!({
                        "format": "some-other-format",
                        "fragments": [],
                    })
                    .to_string(),
                ),
                None,
            )
            .await;

        assert!(result.is_err(), "an unknown format must be rejected");
        assert!(matches!(
            result.unwrap_err(),
            PlaygroundEvaluateError::InvalidInlineSchema(_)
        ));
    }

    /// An envelope with no fragments is an explicitly empty schema
    ///
    /// It is the difference between "there is a schema and it declares
    /// nothing" and "this is not a schema": both are accepted, but they are
    /// not the same thing.
    #[tokio::test]
    async fn test_inline_envelope_without_fragments_is_an_empty_schema() {
        let adapter = SchemaLoaderAdapter::new_inline_only();

        let envelope = crate::internal::engine::schema_codec::encode(&[])
            .expect("an empty schema must be encodable");

        let schema = adapter
            .load_schema(Some(envelope), None)
            .await
            .expect("an empty envelope must be readable");

        assert_eq!(schema.entity_types().count(), 0);
    }
}
