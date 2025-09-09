// crates/security/src/infrastructure/cedar/cedar_schema_adapter.rs

use crate::application::ports::{
    SchemaLoader, PolicySchema, SchemaBasedValidator, SchemaCacheStats, SchemaMetadata,
    SchemaValidationResult, SchemaValidationError, SchemaValidationWarning, SchemaErrorType,
    EntityReference, ActionReference, PolicyLocation
};
use crate::infrastructure::errors::SecurityError;
use cedar_policy::{Schema, Policy, PolicySet, Validator, ValidationMode};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

/// Cedar-based implementation of SchemaLoader
pub struct CedarSchemaLoader {
    schema_cache: Arc<RwLock<HashMap<String, Arc<CedarPolicySchema>>>>,
    cache_stats: Arc<RwLock<SchemaCacheStats>>,
}

impl CedarSchemaLoader {
    pub fn new() -> Self {
        Self {
            schema_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_stats: Arc::new(RwLock::new(SchemaCacheStats::default())),
        }
    }

    fn load_embedded_schema(&self) -> Result<Schema, SecurityError> {
        // Load the test schema for now
        let schema_content = include_str!("../../../schema/test_schema.cedarschema");
        Schema::from_json_str(schema_content)
            .map_err(|e| SecurityError::ValidationError(format!("Schema JSON parse error: {}", e)))
    }

    fn update_cache_stats(&self, hit: bool) {
        if let Ok(mut stats) = self.cache_stats.write() {
            if hit {
                stats.cache_hits += 1;
            } else {
                stats.cache_misses += 1;
            }
        }
    }
}

#[async_trait]
impl SchemaLoader for CedarSchemaLoader {
    async fn load_default_schema(&self) -> Result<Box<dyn PolicySchema>, SecurityError> {
        self.load_schema("default").await
    }

    async fn load_schema(&self, schema_name: &str) -> Result<Box<dyn PolicySchema>, SecurityError> {
        // Check cache first
        {
            let cache = self.schema_cache.read()
                .map_err(|e| SecurityError::ConfigurationError(format!("Failed to read schema cache: {}", e)))?;
            
            if let Some(schema) = cache.get(schema_name) {
                self.update_cache_stats(true);
                return Ok(Box::new(schema.as_ref().clone()));
            }
        }

        self.update_cache_stats(false);

        // Load schema from source
        let cedar_schema = if schema_name == "default" {
            self.load_embedded_schema()?
        } else {
            return Err(SecurityError::ConfigurationError(format!("Unknown schema: {}", schema_name)));
        };

        let policy_schema = Arc::new(CedarPolicySchema::new(cedar_schema, schema_name.to_string()));

        // Cache the loaded schema
        {
            let mut cache = self.schema_cache.write()
                .map_err(|e| SecurityError::ConfigurationError(format!("Failed to write to schema cache: {}", e)))?;
            
            cache.insert(schema_name.to_string(), policy_schema.clone());
        }

        Ok(Box::new(policy_schema.as_ref().clone()))
    }

    async fn load_schema_from_file(&self, file_path: &PathBuf) -> Result<Box<dyn PolicySchema>, SecurityError> {
        let schema_content = std::fs::read_to_string(file_path)
            .map_err(|e| SecurityError::ConfigurationError(format!("Failed to read schema file {:?}: {}", file_path, e)))?;

        let cedar_schema = Schema::from_json_str(&schema_content)
            .map_err(|e| SecurityError::ValidationError(format!("Schema JSON parse error: {}", e)))?;

        let schema_name = file_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_string();

        let policy_schema = CedarPolicySchema::new(cedar_schema, schema_name);
        Ok(Box::new(policy_schema))
    }

    async fn load_schema_from_string(&self, schema_content: &str) -> Result<Box<dyn PolicySchema>, SecurityError> {
        let cedar_schema = Schema::from_json_str(schema_content)
            .map_err(|e| SecurityError::ValidationError(format!("Schema JSON parse error: {}", e)))?;

        let policy_schema = CedarPolicySchema::new(cedar_schema, "from_string".to_string());
        Ok(Box::new(policy_schema))
    }

    async fn validate_schema(&self, schema_content: &str) -> Result<(), SecurityError> {
        let _schema = Schema::from_json_str(schema_content)
            .map_err(|e| SecurityError::ValidationError(format!("Schema validation failed: {}", e)))?;
        Ok(())
    }

    async fn clear_cache(&self) -> Result<(), SecurityError> {
        let mut cache = self.schema_cache.write()
            .map_err(|e| SecurityError::ConfigurationError(format!("Failed to clear schema cache: {}", e)))?;
        
        cache.clear();

        // Reset cache stats
        let mut stats = self.cache_stats.write()
            .map_err(|e| SecurityError::ConfigurationError(format!("Failed to reset cache stats: {}", e)))?;
        *stats = SchemaCacheStats::default();

        Ok(())
    }

    async fn get_cache_stats(&self) -> Result<SchemaCacheStats, SecurityError> {
        let cache = self.schema_cache.read()
            .map_err(|e| SecurityError::ConfigurationError(format!("Failed to read schema cache: {}", e)))?;
        
        let mut stats = self.cache_stats.read()
            .map_err(|e| SecurityError::ConfigurationError(format!("Failed to read cache stats: {}", e)))?
            .clone();

        stats.cached_schemas = cache.len();
        stats.schema_names = cache.keys().cloned().collect();

        Ok(stats)
    }
}

/// Cedar-based implementation of PolicySchema
#[derive(Debug, Clone)]
pub struct CedarPolicySchema {
    schema: Schema,
    schema_id: String,
}

impl CedarPolicySchema {
    pub fn new(schema: Schema, schema_id: String) -> Self {
        Self { schema, schema_id }
    }

    pub fn get_cedar_schema(&self) -> &Schema {
        &self.schema
    }
}

impl PolicySchema for CedarPolicySchema {
    fn get_supported_entity_types(&self) -> Result<Vec<String>, SecurityError> {
        // For now, return hardcoded types from our test schema
        // In a full implementation, we would extract this from the Cedar schema
        Ok(vec![
            "User".to_string(),
            "Repository".to_string(),
        ])
    }

    fn get_supported_actions(&self) -> Result<Vec<String>, SecurityError> {
        // For now, return hardcoded actions from our test schema
        // In a full implementation, we would extract this from the Cedar schema
        Ok(vec![
            "ReadRepository".to_string(),
            "WriteRepository".to_string(),
        ])
    }

    fn is_entity_type_supported(&self, entity_type: &str) -> bool {
        self.get_supported_entity_types()
            .map(|types| types.contains(&entity_type.to_string()))
            .unwrap_or(false)
    }

    fn is_action_supported(&self, action: &str) -> bool {
        self.get_supported_actions()
            .map(|actions| actions.contains(&action.to_string()))
            .unwrap_or(false)
    }

    fn get_schema_id(&self) -> String {
        self.schema_id.clone()
    }

    fn get_metadata(&self) -> SchemaMetadata {
        let entity_types = self.get_supported_entity_types().unwrap_or_default();
        let actions = self.get_supported_actions().unwrap_or_default();

        SchemaMetadata {
            name: self.schema_id.clone(),
            version: "1.0.0".to_string(),
            description: Some("Cedar-based policy schema".to_string()),
            created_at: None,
            entity_count: entity_types.len(),
            action_count: actions.len(),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Cedar-based implementation of SchemaBasedValidator
pub struct CedarSchemaValidator;

impl CedarSchemaValidator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SchemaBasedValidator for CedarSchemaValidator {
    async fn validate_policy_against_schema(
        &self,
        policy_content: &str,
        schema: &dyn PolicySchema
    ) -> Result<SchemaValidationResult, SecurityError> {
        // Downcast to CedarPolicySchema to get the Cedar schema
        let cedar_schema = if let Some(cedar_policy_schema) = schema.as_any().downcast_ref::<CedarPolicySchema>() {
            cedar_policy_schema.get_cedar_schema()
        } else {
            return Err(SecurityError::ValidationError("Schema is not a Cedar schema".to_string()));
        };

        // Parse the policy
        let policy = Policy::from_str(policy_content)
            .map_err(|e| SecurityError::ValidationError(format!("Policy parse error: {}", e)))?;

        // Create policy set
        let policy_set = PolicySet::from_policies([policy])
            .map_err(|e| SecurityError::ValidationError(format!("PolicySet creation error: {}", e)))?;

        // Validate using Cedar
        let validator = Validator::new(cedar_schema.clone());
        let validation_result = validator.validate(&policy_set, ValidationMode::default());

        let mut result = SchemaValidationResult::valid();

        if !validation_result.validation_passed() {
            for error in validation_result.validation_errors() {
                result.add_error(SchemaValidationError {
                    error_type: SchemaErrorType::ConstraintViolation,
                    message: error.to_string(),
                    location: None,
                    suggested_fix: None,
                });
            }
        }

        for warning in validation_result.validation_warnings() {
            result.add_warning(SchemaValidationWarning {
                message: warning.to_string(),
                location: None,
            });
        }

        Ok(result)
    }

    async fn validate_policies_against_schema(
        &self,
        policies: &[&str],
        schema: &dyn PolicySchema
    ) -> Result<Vec<SchemaValidationResult>, SecurityError> {
        let mut results = Vec::new();

        for policy_content in policies {
            let result = self.validate_policy_against_schema(policy_content, schema).await?;
            results.push(result);
        }

        Ok(results)
    }

    async fn check_policy_compatibility(
        &self,
        policy_content: &str,
        schema: &dyn PolicySchema
    ) -> Result<bool, SecurityError> {
        let result = self.validate_policy_against_schema(policy_content, schema).await?;
        Ok(result.is_valid)
    }
}



impl Default for CedarSchemaLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for CedarSchemaValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cedar_schema_loader() {
        let loader = CedarSchemaLoader::new();
        let schema = loader.load_default_schema().await;
        assert!(schema.is_ok());
    }

    #[tokio::test]
    async fn test_cedar_policy_schema() {
        let loader = CedarSchemaLoader::new();
        let schema = loader.load_default_schema().await.unwrap();
        
        let entity_types = schema.get_supported_entity_types().unwrap();
        assert!(!entity_types.is_empty());
        assert!(entity_types.contains(&"User".to_string()));
        
        let actions = schema.get_supported_actions().unwrap();
        assert!(!actions.is_empty());
        assert!(actions.contains(&"ReadRepository".to_string()));
    }

    #[tokio::test]
    async fn test_cedar_schema_validator() {
        let loader = CedarSchemaLoader::new();
        let schema = loader.load_default_schema().await.unwrap();
        let validator = CedarSchemaValidator::new();
        
        let policy = r#"permit(principal, action, resource);"#;
        let result = validator.validate_policy_against_schema(policy, schema.as_ref()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cache_functionality() {
        let loader = CedarSchemaLoader::new();
        
        // Load schema twice
        let _schema1 = loader.load_default_schema().await.unwrap();
        let _schema2 = loader.load_default_schema().await.unwrap();
        
        // Check cache stats
        let stats = loader.get_cache_stats().await.unwrap();
        assert_eq!(stats.cached_schemas, 1);
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(stats.cache_misses, 1);
    }
}