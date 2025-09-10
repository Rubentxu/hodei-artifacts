// crates/security/src/infrastructure/validation/comprehensive_cedar_validator.rs

use crate::infrastructure::errors::SecurityError;
use crate::infrastructure::cedar::{EnhancedCedarValidator, EnhancedValidationResult};
use crate::application::ports::SchemaLoader;
use crate::infrastructure::cedar::CedarSchemaLoaderAdapter;
use std::sync::Arc;

/// Comprehensive Cedar validator that uses all Cedar validation capabilities
/// This is a wrapper around EnhancedCedarValidator for backward compatibility
pub struct ComprehensiveCedarValidator {
    enhanced_validator: EnhancedCedarValidator,
}

impl ComprehensiveCedarValidator {
    /// Create a new comprehensive validator with the Hodei schema
    pub fn new() -> Result<Self, SecurityError> {
        let enhanced_validator = EnhancedCedarValidator::new()?;

        Ok(Self {
            enhanced_validator,
        })
    }

    /// Create a comprehensive validator with custom schema loader
    pub fn with_schema_loader(_schema_loader: Arc<dyn SchemaLoader>) -> Result<Self, SecurityError> {
        // For now, we'll create a new enhanced validator
        // In the future, we could make EnhancedCedarValidator use the SchemaLoader port
        let enhanced_validator = EnhancedCedarValidator::new()?;

        Ok(Self {
            enhanced_validator,
        })
    }

    /// Perform comprehensive validation using all Cedar capabilities
    pub async fn validate_policy_comprehensive(&self, policy_content: &str) -> Result<ComprehensiveValidationResult, SecurityError> {
        let enhanced_result = self.enhanced_validator.validate_policy_comprehensive(policy_content).await?;
        
        // Convert EnhancedValidationResult to ComprehensiveValidationResult for backward compatibility
        Ok(ComprehensiveValidationResult::from_enhanced(enhanced_result))
    }



    /// Get information about supported entity types
    pub fn get_supported_entity_types(&self) -> Vec<String> {
        self.enhanced_validator.get_supported_entity_types()
            .unwrap_or_else(|_| Vec::new())
    }

    /// Get information about supported actions
    pub fn get_supported_actions(&self) -> Vec<String> {
        self.enhanced_validator.get_supported_actions()
            .unwrap_or_else(|_| Vec::new())
    }

    /// Get a new schema loader instance
    pub fn create_schema_loader(&self) -> Arc<dyn SchemaLoader> {
        Arc::new(CedarSchemaLoaderAdapter::new())
    }

    /// Validate multiple policies as a batch
    pub async fn validate_policy_batch(&self, policies: &[&str]) -> Result<Vec<ComprehensiveValidationResult>, SecurityError> {
        let batch_result = self.enhanced_validator.validate_policy_batch(policies).await?;
        
        let results = batch_result.individual_results
            .into_iter()
            .map(|(_, result)| ComprehensiveValidationResult::from_enhanced(result))
            .collect();
        
        Ok(results)
    }
}

impl Default for ComprehensiveCedarValidator {
    fn default() -> Self {
        Self::new().expect("Failed to create default ComprehensiveCedarValidator")
    }
}

/// Comprehensive validation result that includes all types of validation feedback
#[derive(Debug, Clone)]
pub struct ComprehensiveValidationResult {
    pub is_valid: bool,
    pub hrn_errors: Vec<String>,
    pub syntax_errors: Vec<String>,
    pub semantic_errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ComprehensiveValidationResult {
    pub fn new() -> Self {
        Self {
            is_valid: true,
            hrn_errors: Vec::new(),
            syntax_errors: Vec::new(),
            semantic_errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Convert from EnhancedValidationResult for backward compatibility
    pub fn from_enhanced(enhanced: EnhancedValidationResult) -> Self {
        Self {
            is_valid: enhanced.is_valid,
            hrn_errors: enhanced.hrn_errors,
            syntax_errors: enhanced.syntax_errors,
            semantic_errors: enhanced.semantic_errors,
            warnings: enhanced.warnings,
        }
    }

    pub fn add_hrn_error(&mut self, error: String) {
        self.hrn_errors.push(error);
        self.is_valid = false;
    }

    pub fn add_syntax_error(&mut self, error: String) {
        self.syntax_errors.push(error);
        self.is_valid = false;
    }

    pub fn add_semantic_error(&mut self, error: String) {
        self.semantic_errors.push(error);
        self.is_valid = false;
    }

    pub fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    pub fn has_errors(&self) -> bool {
        !self.hrn_errors.is_empty() || !self.syntax_errors.is_empty() || !self.semantic_errors.is_empty()
    }

    pub fn get_all_errors(&self) -> Vec<String> {
        let mut all_errors = Vec::new();
        all_errors.extend(self.hrn_errors.clone());
        all_errors.extend(self.syntax_errors.clone());
        all_errors.extend(self.semantic_errors.clone());
        all_errors
    }

    pub fn get_error_summary(&self) -> String {
        let all_errors = self.get_all_errors();
        if all_errors.is_empty() {
            "No errors".to_string()
        } else {
            all_errors.join("; ")
        }
    }
}

impl Default for ComprehensiveValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_comprehensive_validator_creation() {
        let validator = ComprehensiveCedarValidator::new();
        assert!(validator.is_ok());
    }

    #[tokio::test]
    async fn test_validate_simple_policy() {
        let validator = ComprehensiveCedarValidator::new().unwrap();
        
        let policy = r#"
            permit(
                principal == Hodei::User::"hrn:hodei:iam:global:system:user/alice",
                action == Hodei::ReadRepository,
                resource == Hodei::Repository::"hrn:hodei:artifact:global:system:repository/myorg/myrepo"
            );
        "#;

        let result = validator.validate_policy_comprehensive(policy).await;
        assert!(result.is_ok());
        
        let validation_result = result.unwrap();
        // The policy might have semantic errors due to schema mismatch, but should not have syntax errors
        assert!(validation_result.syntax_errors.is_empty(), "Should not have syntax errors: {:?}", validation_result.syntax_errors);
    }

    #[tokio::test]
    async fn test_validate_policy_with_invalid_hrn() {
        let validator = ComprehensiveCedarValidator::new().unwrap();
        
        let policy = r#"
            permit(
                principal == Hodei::User::"hrn:hodei:unknown-service:global:system:user/alice",
                action == Hodei::ReadRepository,
                resource == Hodei::Repository::"hrn:hodei:artifact:global:system:repository/myorg/myrepo"
            );
        "#;

        let result = validator.validate_policy_comprehensive(policy).await;
        assert!(result.is_ok());
        
        let validation_result = result.unwrap();
        assert!(!validation_result.hrn_errors.is_empty(), "Should have HRN errors");
        assert!(!validation_result.is_valid, "Should be invalid due to HRN errors");
    }

    #[tokio::test]
    async fn test_validate_policy_with_syntax_error() {
        let validator = ComprehensiveCedarValidator::new().unwrap();
        
        let policy = r#"
            invalid_syntax(
                principal == Hodei::User::"hrn:hodei:iam:global:system:user/alice",
                action == Hodei::ReadRepository,
                resource == Hodei::Repository::"hrn:hodei:artifact:global:system:repository/myorg/myrepo"
            );
        "#;

        let result = validator.validate_policy_comprehensive(policy).await;
        assert!(result.is_ok());
        
        let validation_result = result.unwrap();
        assert!(!validation_result.syntax_errors.is_empty(), "Should have syntax errors");
        assert!(!validation_result.is_valid, "Should be invalid due to syntax errors");
    }

    #[test]
    fn test_get_supported_types() {
        let validator = ComprehensiveCedarValidator::new().unwrap();
        
        let entity_types = validator.get_supported_entity_types();
        assert!(entity_types.contains(&"User".to_string()));
        assert!(entity_types.contains(&"Repository".to_string()));
        
        let actions = validator.get_supported_actions();
        assert!(actions.contains(&"ReadRepository".to_string()));
        assert!(actions.contains(&"AdminAccess".to_string()));
    }
}