// crates/security/src/application/ports/schema_ports.rs

use crate::infrastructure::errors::SecurityError;
use async_trait::async_trait;
use std::path::PathBuf;

/// Port for schema loading operations
#[async_trait]
pub trait SchemaLoader: Send + Sync {
    /// Load the default schema
    async fn load_default_schema(&self) -> Result<Box<dyn PolicySchema>, SecurityError>;
    
    /// Load a schema by name
    async fn load_schema(&self, schema_name: &str) -> Result<Box<dyn PolicySchema>, SecurityError>;
    
    /// Load schema from file path
    async fn load_schema_from_file(&self, file_path: &PathBuf) -> Result<Box<dyn PolicySchema>, SecurityError>;
    
    /// Load schema from string content
    async fn load_schema_from_string(&self, schema_content: &str) -> Result<Box<dyn PolicySchema>, SecurityError>;
    
    /// Validate schema content without loading
    async fn validate_schema(&self, schema_content: &str) -> Result<(), SecurityError>;
    
    /// Clear schema cache
    async fn clear_cache(&self) -> Result<(), SecurityError>;
    
    /// Get cache statistics
    async fn get_cache_stats(&self) -> Result<SchemaCacheStats, SecurityError>;
}

/// Port for schema information and metadata
pub trait PolicySchema: Send + Sync {
    /// Get supported entity types
    fn get_supported_entity_types(&self) -> Result<Vec<String>, SecurityError>;
    
    /// Get supported actions
    fn get_supported_actions(&self) -> Result<Vec<String>, SecurityError>;
    
    /// Check if an entity type is supported
    fn is_entity_type_supported(&self, entity_type: &str) -> bool;
    
    /// Check if an action is supported
    fn is_action_supported(&self, action: &str) -> bool;
    
    /// Get schema version or identifier
    fn get_schema_id(&self) -> String;
    
    /// Get schema metadata
    fn get_metadata(&self) -> SchemaMetadata;
    
    /// Enable downcasting for implementation-specific operations
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Port for policy validation using schema
#[async_trait]
pub trait SchemaBasedValidator: Send + Sync {
    /// Validate a policy against the schema
    async fn validate_policy_against_schema(
        &self, 
        policy_content: &str, 
        schema: &dyn PolicySchema
    ) -> Result<SchemaValidationResult, SecurityError>;
    
    /// Validate multiple policies against the schema
    async fn validate_policies_against_schema(
        &self, 
        policies: &[&str], 
        schema: &dyn PolicySchema
    ) -> Result<Vec<SchemaValidationResult>, SecurityError>;
    
    /// Check if a policy is compatible with the schema
    async fn check_policy_compatibility(
        &self, 
        policy_content: &str, 
        schema: &dyn PolicySchema
    ) -> Result<bool, SecurityError>;
}

/// Schema cache statistics
#[derive(Debug, Clone)]
pub struct SchemaCacheStats {
    pub cached_schemas: usize,
    pub schema_names: Vec<String>,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

/// Schema metadata information
#[derive(Debug, Clone)]
pub struct SchemaMetadata {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub created_at: Option<String>,
    pub entity_count: usize,
    pub action_count: usize,
}

/// Result of schema-based validation
#[derive(Debug, Clone)]
pub struct SchemaValidationResult {
    pub is_valid: bool,
    pub schema_errors: Vec<SchemaValidationError>,
    pub warnings: Vec<SchemaValidationWarning>,
    pub entity_references: Vec<EntityReference>,
    pub action_references: Vec<ActionReference>,
}

/// Schema validation error
#[derive(Debug, Clone)]
pub struct SchemaValidationError {
    pub error_type: SchemaErrorType,
    pub message: String,
    pub location: Option<PolicyLocation>,
    pub suggested_fix: Option<String>,
}

/// Schema validation warning
#[derive(Debug, Clone)]
pub struct SchemaValidationWarning {
    pub message: String,
    pub location: Option<PolicyLocation>,
}

/// Types of schema validation errors
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaErrorType {
    UnknownEntityType,
    UnknownAction,
    UnknownAttribute,
    InvalidEntityReference,
    InvalidActionApplication,
    TypeMismatch,
    ConstraintViolation,
}

/// Reference to an entity in a policy
#[derive(Debug, Clone)]
pub struct EntityReference {
    pub entity_type: String,
    pub entity_id: String,
    pub location: Option<PolicyLocation>,
}

/// Reference to an action in a policy
#[derive(Debug, Clone)]
pub struct ActionReference {
    pub action_name: String,
    pub location: Option<PolicyLocation>,
}

/// Location in a policy
#[derive(Debug, Clone)]
pub struct PolicyLocation {
    pub line: u32,
    pub column: u32,
}

impl SchemaValidationResult {
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            schema_errors: Vec::new(),
            warnings: Vec::new(),
            entity_references: Vec::new(),
            action_references: Vec::new(),
        }
    }

    pub fn invalid(errors: Vec<SchemaValidationError>) -> Self {
        Self {
            is_valid: false,
            schema_errors: errors,
            warnings: Vec::new(),
            entity_references: Vec::new(),
            action_references: Vec::new(),
        }
    }

    pub fn add_error(&mut self, error: SchemaValidationError) {
        self.schema_errors.push(error);
        self.is_valid = false;
    }

    pub fn add_warning(&mut self, warning: SchemaValidationWarning) {
        self.warnings.push(warning);
    }

    pub fn get_error_summary(&self) -> String {
        self.schema_errors
            .iter()
            .map(|e| &e.message)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ")
    }
}

impl Default for SchemaCacheStats {
    fn default() -> Self {
        Self {
            cached_schemas: 0,
            schema_names: Vec::new(),
            cache_hits: 0,
            cache_misses: 0,
        }
    }
}

impl Default for SchemaMetadata {
    fn default() -> Self {
        Self {
            name: "unknown".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            created_at: None,
            entity_count: 0,
            action_count: 0,
        }
    }
}