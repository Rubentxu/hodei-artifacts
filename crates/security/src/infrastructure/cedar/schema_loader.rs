// crates/security/src/infrastructure/cedar/schema_loader.rs

use crate::infrastructure::errors::SecurityError;
use cedar_policy::{Schema, SchemaFragment};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

/// Cedar schema loader with caching capabilities
/// Provides centralized schema management for all Cedar validation operations
pub struct CedarSchemaLoader {
    /// Cache of loaded schemas by name
    schema_cache: Arc<RwLock<HashMap<String, Schema>>>,
    /// Default schema path
    default_schema_path: PathBuf,
}

impl CedarSchemaLoader {
    /// Create a new schema loader with default schema path
    pub fn new() -> Self {
        Self {
            schema_cache: Arc::new(RwLock::new(HashMap::new())),
            default_schema_path: PathBuf::from("schema/policy_schema.cedarschema"),
        }
    }

    /// Create a schema loader with custom default path
    pub fn with_default_path(path: PathBuf) -> Self {
        Self {
            schema_cache: Arc::new(RwLock::new(HashMap::new())),
            default_schema_path: path,
        }
    }

    /// Load the default Hodei schema
    pub fn load_default_schema(&self) -> Result<Schema, SecurityError> {
        self.load_schema("default")
    }

    /// Load a schema by name (uses caching)
    pub fn load_schema(&self, schema_name: &str) -> Result<Schema, SecurityError> {
        // Check cache first
        {
            let cache = self.schema_cache.read().map_err(|e| {
                SecurityError::ConfigurationError(format!("Failed to read schema cache: {}", e))
            })?;

            if let Some(schema) = cache.get(schema_name) {
                return Ok(schema.clone());
            }
        }

        // Load schema from source
        let schema = if schema_name == "default" {
            self.load_schema_from_embedded()?
        } else {
            return Err(SecurityError::ConfigurationError(format!(
                "Unknown schema: {}",
                schema_name
            )));
        };

        // Cache the loaded schema
        {
            let mut cache = self.schema_cache.write().map_err(|e| {
                SecurityError::ConfigurationError(format!("Failed to write to schema cache: {}", e))
            })?;

            cache.insert(schema_name.to_string(), schema.clone());
        }

        Ok(schema)
    }

    /// Load schema from embedded content (the .cedarschema file)
    fn load_schema_from_embedded(&self) -> Result<Schema, SecurityError> {
        // Load the Cedar schema from the embedded .cedarschema file
        // Using test schema for now - this is JSON format schema
        let schema_content = include_str!("../../../schema/test_schema.cedarschema");

        // Parse the Cedar schema using Cedar's JSON schema parser
        let schema = Schema::from_json_str(schema_content).map_err(|e| {
            SecurityError::ValidationError(format!("Schema JSON parse error: {}", e))
        })?;

        Ok(schema)
    }

    /// Load schema from file path (expects JSON format)
    pub fn load_schema_from_file(&self, file_path: &PathBuf) -> Result<Schema, SecurityError> {
        let schema_content = std::fs::read_to_string(file_path).map_err(|e| {
            SecurityError::ConfigurationError(format!(
                "Failed to read schema file {:?}: {}",
                file_path, e
            ))
        })?;

        let schema = Schema::from_json_str(&schema_content).map_err(|e| {
            SecurityError::ValidationError(format!("Schema JSON parse error: {}", e))
        })?;

        Ok(schema)
    }

    /// Load schema from string content (expects JSON format)
    pub fn load_schema_from_string(&self, schema_content: &str) -> Result<Schema, SecurityError> {
        let schema = Schema::from_json_str(schema_content).map_err(|e| {
            SecurityError::ValidationError(format!("Schema JSON parse error: {}", e))
        })?;

        Ok(schema)
    }

    /// Validate that a schema is well-formed
    pub fn validate_schema(&self, schema_content: &str) -> Result<(), SecurityError> {
        // Try to parse the schema to validate it
        let _schema = self.load_schema_from_string(schema_content)?;
        Ok(())
    }

    /// Clear the schema cache
    pub fn clear_cache(&self) -> Result<(), SecurityError> {
        let mut cache = self.schema_cache.write().map_err(|e| {
            SecurityError::ConfigurationError(format!("Failed to clear schema cache: {}", e))
        })?;

        cache.clear();
        Ok(())
    }

    /// Get cache statistics
    pub fn get_cache_stats(&self) -> Result<CacheStats, SecurityError> {
        let cache = self.schema_cache.read().map_err(|e| {
            SecurityError::ConfigurationError(format!("Failed to read schema cache: {}", e))
        })?;

        Ok(CacheStats {
            cached_schemas: cache.len(),
            schema_names: cache.keys().cloned().collect(),
        })
    }

    /// Reload a specific schema (clears from cache and reloads)
    pub fn reload_schema(&self, schema_name: &str) -> Result<Schema, SecurityError> {
        // Remove from cache
        {
            let mut cache = self.schema_cache.write().map_err(|e| {
                SecurityError::ConfigurationError(format!("Failed to write to schema cache: {}", e))
            })?;

            cache.remove(schema_name);
        }

        // Load fresh copy
        self.load_schema(schema_name)
    }

    /// Get information about supported entity types from the default schema
    pub fn get_supported_entity_types(&self) -> Result<Vec<String>, SecurityError> {
        let _schema = self.load_default_schema()?;

        // For now, return the known entity types from our schema
        // In a more advanced implementation, we could extract this from the schema directly
        Ok(vec![
            "User".to_string(),
            "Policy".to_string(),
            "ApiKey".to_string(),
            "ServiceAccount".to_string(),
            "Organization".to_string(),
            "Team".to_string(),
            "Membership".to_string(),
            "Repository".to_string(),
            "PhysicalArtifact".to_string(),
            "PackageVersion".to_string(),
            "ArtifactMetadata".to_string(),
            "Configuration".to_string(),
            "ConfigTemplate".to_string(),
            "ConfigVersion".to_string(),
            "Event".to_string(),
            "EventStream".to_string(),
            "EventSubscription".to_string(),
            "Metric".to_string(),
            "Dashboard".to_string(),
            "Report".to_string(),
            "Alert".to_string(),
            "Attestation".to_string(),
            "PublicKey".to_string(),
            "ScanResult".to_string(),
            "VulnerabilityDefinition".to_string(),
            "VulnerabilityOccurrence".to_string(),
            "ProvenanceRecord".to_string(),
            "StorageBackend".to_string(),
            "StorageBucket".to_string(),
            "StoragePolicy".to_string(),
            "Monitor".to_string(),
            "HealthCheck".to_string(),
            "LogStream".to_string(),
        ])
    }

    /// Get information about supported actions from the default schema
    pub fn get_supported_actions(&self) -> Result<Vec<String>, SecurityError> {
        let _schema = self.load_default_schema()?;

        // For now, return the known actions from our schema
        // In a more advanced implementation, we could extract this from the schema directly
        Ok(vec![
            "CreateUser".to_string(),
            "ReadUser".to_string(),
            "UpdateUser".to_string(),
            "DeleteUser".to_string(),
            "ManagePolicies".to_string(),
            "ManageApiKeys".to_string(),
            "CreateOrganization".to_string(),
            "ManageOrganization".to_string(),
            "ManageTeams".to_string(),
            "ManageMemberships".to_string(),
            "CreateRepository".to_string(),
            "ReadRepository".to_string(),
            "WriteRepository".to_string(),
            "DeleteRepository".to_string(),
            "ReadArtifact".to_string(),
            "WriteArtifact".to_string(),
            "DeleteArtifact".to_string(),
            "ManageMetadata".to_string(),
            "ReadConfiguration".to_string(),
            "WriteConfiguration".to_string(),
            "ManageConfigVersions".to_string(),
            "PublishEvent".to_string(),
            "ReadEvent".to_string(),
            "ManageEventStreams".to_string(),
            "ManageSubscriptions".to_string(),
            "ReadMetrics".to_string(),
            "CreateDashboard".to_string(),
            "ManageDashboards".to_string(),
            "GenerateReports".to_string(),
            "ManageAlerts".to_string(),
            "CreateAttestation".to_string(),
            "ReadAttestation".to_string(),
            "ManageKeys".to_string(),
            "RunScans".to_string(),
            "ReadScanResults".to_string(),
            "ManageProvenance".to_string(),
            "ManageStorage".to_string(),
            "ReadStorage".to_string(),
            "WriteStorage".to_string(),
            "CreateMonitor".to_string(),
            "ReadMonitor".to_string(),
            "ManageMonitors".to_string(),
            "ReadLogs".to_string(),
            "AdminAccess".to_string(),
            "SystemAccess".to_string(),
        ])
    }
}

impl Default for CedarSchemaLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the schema cache
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub cached_schemas: usize,
    pub schema_names: Vec<String>,
}

/// Configuration for schema loading
#[derive(Debug, Clone)]
pub struct SchemaConfig {
    pub enable_caching: bool,
    pub cache_ttl_seconds: Option<u64>,
    pub schema_paths: HashMap<String, PathBuf>,
}

impl Default for SchemaConfig {
    fn default() -> Self {
        Self {
            enable_caching: true,
            cache_ttl_seconds: None, // No TTL by default
            schema_paths: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_loader_creation() {
        let loader = CedarSchemaLoader::new();
        assert!(loader.schema_cache.read().unwrap().is_empty());
    }

    #[test]
    fn test_load_default_schema() {
        let loader = CedarSchemaLoader::new();
        let result = loader.load_default_schema();
        assert!(
            result.is_ok(),
            "Should be able to load default schema: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_schema_caching() {
        let loader = CedarSchemaLoader::new();

        // Load schema twice
        let schema1 = loader.load_default_schema().unwrap();
        let schema2 = loader.load_default_schema().unwrap();

        // Should be cached (same instance)
        // Note: Schema doesn't implement PartialEq, so we can't directly compare
        // But we can check that cache has the entry
        let stats = loader.get_cache_stats().unwrap();
        assert_eq!(stats.cached_schemas, 1);
        assert!(stats.schema_names.contains(&"default".to_string()));
    }

    #[test]
    fn test_cache_clear() {
        let loader = CedarSchemaLoader::new();

        // Load schema to populate cache
        let _schema = loader.load_default_schema().unwrap();

        // Verify cache has content
        let stats = loader.get_cache_stats().unwrap();
        assert_eq!(stats.cached_schemas, 1);

        // Clear cache
        loader.clear_cache().unwrap();

        // Verify cache is empty
        let stats = loader.get_cache_stats().unwrap();
        assert_eq!(stats.cached_schemas, 0);
    }

    #[test]
    fn test_schema_validation() {
        let loader = CedarSchemaLoader::new();

        // Valid schema content
        let valid_schema = r#"
            namespace Test {
                entity User = {
                    "name": String,
                };
                
                action ReadUser appliesTo {
                    principal: [User],
                    resource: [User]
                };
            }
        "#;

        let result = loader.validate_schema(valid_schema);
        assert!(
            result.is_ok(),
            "Valid schema should pass validation: {:?}",
            result.err()
        );

        // Invalid schema content
        let invalid_schema = "invalid schema content";
        let result = loader.validate_schema(invalid_schema);
        assert!(result.is_err(), "Invalid schema should fail validation");
    }

    #[test]
    fn test_get_supported_types() {
        let loader = CedarSchemaLoader::new();

        let entity_types = loader.get_supported_entity_types().unwrap();
        assert!(!entity_types.is_empty());
        assert!(entity_types.contains(&"User".to_string()));
        assert!(entity_types.contains(&"Repository".to_string()));

        let actions = loader.get_supported_actions().unwrap();
        assert!(!actions.is_empty());
        assert!(actions.contains(&"ReadRepository".to_string()));
        assert!(actions.contains(&"AdminAccess".to_string()));
    }

    #[test]
    fn test_reload_schema() {
        let loader = CedarSchemaLoader::new();

        // Load schema initially
        let _schema1 = loader.load_default_schema().unwrap();

        // Verify it's cached
        let stats = loader.get_cache_stats().unwrap();
        assert_eq!(stats.cached_schemas, 1);

        // Reload schema
        let _schema2 = loader.reload_schema("default").unwrap();

        // Should still be cached (reloaded)
        let stats = loader.get_cache_stats().unwrap();
        assert_eq!(stats.cached_schemas, 1);
    }
}
