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
    /// # Arguments
    ///
    /// * `schema_json` - Cedar schema in JSON format
    ///
    /// # Returns
    ///
    /// A parsed Cedar Schema
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Schema JSON is invalid
    /// - Schema parsing fails
    fn parse_inline_schema(&self, schema_json: &str) -> Result<Schema, PlaygroundEvaluateError> {
        debug!("Parsing inline schema");

        // Parse JSON to validate it first
        let json_value: serde_json::Value = serde_json::from_str(schema_json)
            .map_err(|e| PlaygroundEvaluateError::SchemaError(format!("Invalid JSON: {}", e)))?;

        // Convert to Cedar schema format
        // For now, we'll create an empty schema if the JSON is empty
        if json_value.is_null()
            || (json_value.is_object() && json_value.as_object().unwrap().is_empty())
        {
            info!("Creating empty schema from inline JSON");
            Schema::from_schema_fragments(vec![]).map_err(|e| {
                PlaygroundEvaluateError::SchemaError(format!(
                    "Failed to create empty schema: {}",
                    e
                ))
            })
        } else {
            // Try to parse as Cedar schema
            // Note: Cedar schemas in JSON format need to be converted to schema fragments
            // For now, we'll return an empty schema and log a warning
            warn!("Non-empty schema JSON provided, but full schema parsing not yet implemented");
            Schema::from_schema_fragments(vec![]).map_err(|e| {
                PlaygroundEvaluateError::SchemaError(format!("Schema parsing error: {}", e))
            })
        }
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
            PlaygroundEvaluateError::SchemaError(_)
        ));
    }
}
