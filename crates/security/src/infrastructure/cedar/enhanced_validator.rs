// crates/security/src/infrastructure/cedar/enhanced_validator.rs

use crate::infrastructure::cedar::schema_loader::CedarSchemaLoader;
use crate::infrastructure::errors::SecurityError;
use crate::infrastructure::validation::HrnValidator;
use cedar_policy::{Policy, PolicySet, Validator, ValidationMode};
use std::str::FromStr;
use std::sync::Arc;

/// Enhanced Cedar validator that uses the shared schema infrastructure
/// Provides comprehensive validation with caching and performance optimization
pub struct EnhancedCedarValidator {
    schema_loader: Arc<CedarSchemaLoader>,
    hrn_validator: HrnValidator,
}

impl EnhancedCedarValidator {
    /// Create a new enhanced validator with default schema loader
    pub fn new() -> Result<Self, SecurityError> {
        let schema_loader = Arc::new(CedarSchemaLoader::new());
        let hrn_validator = HrnValidator::new();

        Ok(Self {
            schema_loader,
            hrn_validator,
        })
    }

    /// Create a new enhanced validator with custom schema loader
    pub fn with_schema_loader(schema_loader: Arc<CedarSchemaLoader>) -> Self {
        let hrn_validator = HrnValidator::new();

        Self {
            schema_loader,
            hrn_validator,
        }
    }

    /// Perform comprehensive validation using cached schema
    pub async fn validate_policy_comprehensive(&self, policy_content: &str) -> Result<EnhancedValidationResult, SecurityError> {
        let mut result = EnhancedValidationResult::new();

        // Step 1: HRN Validation
        if let Err(e) = self.hrn_validator.validate_policy_hrns(policy_content) {
            result.add_hrn_error(e.to_string());
        }

        // Step 2: Syntax Validation - Parse the policy
        let policy = match Policy::from_str(policy_content) {
            Ok(p) => p,
            Err(e) => {
                result.add_syntax_error(format!("Policy syntax error: {}", e));
                return Ok(result); // Return early if syntax is invalid
            }
        };

        // Step 3: Create PolicySet for validation
        let policy_set = match PolicySet::from_policies([policy]) {
            Ok(ps) => ps,
            Err(e) => {
                result.add_syntax_error(format!("PolicySet creation error: {}", e));
                return Ok(result);
            }
        };

        // Step 4: Schema-based Semantic Validation using cached schema
        let schema = self.schema_loader.load_default_schema()
            .map_err(|e| SecurityError::ValidationError(format!("Failed to load schema: {}", e)))?;
        
        let validator = Validator::new(schema);
        
        // Use Cedar's comprehensive validation
        let validation_result = validator.validate(&policy_set, ValidationMode::default());
        
        if !validation_result.validation_passed() {
            for error in validation_result.validation_errors() {
                result.add_semantic_error(error.to_string());
            }
        }

        // Collect warnings as well
        for warning in validation_result.validation_warnings() {
            result.add_warning(warning.to_string());
        }

        // Step 5: Additional structural validations
        self.validate_policy_structure(&policy_set, &mut result)?;

        Ok(result)
    }

    /// Validate multiple policies as a batch
    pub async fn validate_policy_batch(&self, policies: &[&str]) -> Result<BatchValidationResult, SecurityError> {
        let mut batch_result = BatchValidationResult::new();

        // Validate each policy individually
        for (index, policy_content) in policies.iter().enumerate() {
            let individual_result = self.validate_policy_comprehensive(policy_content).await?;
            batch_result.add_individual_result(index, individual_result);
        }

        // Validate the combined policy set for conflicts
        if policies.len() > 1 {
            let combined_policies = policies.join("\n");
            let combined_result = self.validate_policy_comprehensive(&combined_policies).await?;
            batch_result.set_combined_result(combined_result);
        }

        Ok(batch_result)
    }

    /// Validate policy structure using Cedar's analysis capabilities
    fn validate_policy_structure(&self, policy_set: &PolicySet, result: &mut EnhancedValidationResult) -> Result<(), SecurityError> {
        let policies: Vec<_> = policy_set.policies().collect();
        
        // Check for policy completeness
        for policy in policies {
            self.validate_policy_completeness(policy, result);
        }

        Ok(())
    }

    /// Validate that a policy is complete and well-formed
    fn validate_policy_completeness(&self, policy: &Policy, result: &mut EnhancedValidationResult) {
        let policy_str = policy.to_string();
        
        // Check if the policy has meaningful conditions
        if policy_str.contains("principal") && policy_str.contains("action") && policy_str.contains("resource") {
            // Policy has the basic components
        } else {
            result.add_warning(format!(
                "Policy {} may be incomplete - missing principal, action, or resource constraints",
                policy.id()
            ));
        }

        // Check for overly permissive policies
        if policy_str.contains("permit") && !policy_str.contains("when") && !policy_str.contains("unless") {
            result.add_warning(format!(
                "Policy {} is very permissive - consider adding conditions",
                policy.id()
            ));
        }
    }

    /// Get the schema loader for advanced operations
    pub fn get_schema_loader(&self) -> Arc<CedarSchemaLoader> {
        self.schema_loader.clone()
    }

    /// Get information about supported entity types
    pub fn get_supported_entity_types(&self) -> Result<Vec<String>, SecurityError> {
        self.schema_loader.get_supported_entity_types()
    }

    /// Get information about supported actions
    pub fn get_supported_actions(&self) -> Result<Vec<String>, SecurityError> {
        self.schema_loader.get_supported_actions()
    }

    /// Validate schema content
    pub fn validate_schema(&self, schema_content: &str) -> Result<(), SecurityError> {
        self.schema_loader.validate_schema(schema_content)
    }

    /// Get cache statistics
    pub fn get_cache_stats(&self) -> Result<crate::infrastructure::cedar::CacheStats, SecurityError> {
        self.schema_loader.get_cache_stats()
    }

    /// Clear schema cache
    pub fn clear_cache(&self) -> Result<(), SecurityError> {
        self.schema_loader.clear_cache()
    }
}

impl Default for EnhancedCedarValidator {
    fn default() -> Self {
        Self::new().expect("Failed to create default EnhancedCedarValidator")
    }
}

/// Enhanced validation result with detailed categorization
#[derive(Debug, Clone)]
pub struct EnhancedValidationResult {
    pub is_valid: bool,
    pub hrn_errors: Vec<String>,
    pub syntax_errors: Vec<String>,
    pub semantic_errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl EnhancedValidationResult {
    pub fn new() -> Self {
        Self {
            is_valid: true,
            hrn_errors: Vec::new(),
            syntax_errors: Vec::new(),
            semantic_errors: Vec::new(),
            warnings: Vec::new(),
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

impl Default for EnhancedValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Result for batch validation operations
#[derive(Debug, Clone)]
pub struct BatchValidationResult {
    pub overall_valid: bool,
    pub individual_results: Vec<(usize, EnhancedValidationResult)>,
    pub combined_result: Option<EnhancedValidationResult>,
}

impl BatchValidationResult {
    pub fn new() -> Self {
        Self {
            overall_valid: true,
            individual_results: Vec::new(),
            combined_result: None,
        }
    }

    pub fn add_individual_result(&mut self, index: usize, result: EnhancedValidationResult) {
        if !result.is_valid {
            self.overall_valid = false;
        }
        self.individual_results.push((index, result));
    }

    pub fn set_combined_result(&mut self, result: EnhancedValidationResult) {
        if !result.is_valid {
            self.overall_valid = false;
        }
        self.combined_result = Some(result);
    }

    pub fn get_summary(&self) -> String {
        let total_policies = self.individual_results.len();
        let valid_policies = self.individual_results.iter()
            .filter(|(_, result)| result.is_valid)
            .count();
        
        format!("Batch validation: {}/{} policies valid, overall: {}", 
                valid_policies, total_policies, self.overall_valid)
    }
}

impl Default for BatchValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_enhanced_validator_creation() {
        let validator = EnhancedCedarValidator::new();
        assert!(validator.is_ok());
    }

    #[tokio::test]
    async fn test_validate_simple_policy() {
        let validator = EnhancedCedarValidator::new().unwrap();
        
        let policy = r#"
            permit(
                principal == User::"alice",
                action == Action::"ReadRepository", 
                resource == Repository::"myrepo"
            );
        "#;

        let result = validator.validate_policy_comprehensive(policy).await;
        assert!(result.is_ok());
        
        let validation_result = result.unwrap();
        // The policy should have valid syntax
        assert!(validation_result.syntax_errors.is_empty(), "Should not have syntax errors: {:?}", validation_result.syntax_errors);
    }

    #[tokio::test]
    async fn test_batch_validation() {
        let validator = EnhancedCedarValidator::new().unwrap();
        
        let policies = vec![
            r#"permit(principal, action, resource);"#,
            r#"forbid(principal, action, resource);"#,
        ];

        let result = validator.validate_policy_batch(&policies).await;
        assert!(result.is_ok());
        
        let batch_result = result.unwrap();
        assert_eq!(batch_result.individual_results.len(), 2);
    }

    #[tokio::test]
    async fn test_get_supported_types() {
        let validator = EnhancedCedarValidator::new().unwrap();
        
        let entity_types = validator.get_supported_entity_types().unwrap();
        assert!(!entity_types.is_empty());
        assert!(entity_types.contains(&"User".to_string()));
        
        let actions = validator.get_supported_actions().unwrap();
        assert!(!actions.is_empty());
        assert!(actions.contains(&"ReadRepository".to_string()));
    }

    #[tokio::test]
    async fn test_cache_operations() {
        let validator = EnhancedCedarValidator::new().unwrap();
        
        // Load schema to populate cache
        let _result = validator.validate_policy_comprehensive("permit(principal, action, resource);").await.unwrap();
        
        // Check cache stats
        let stats = validator.get_cache_stats().unwrap();
        assert!(stats.cached_schemas > 0);
        
        // Clear cache
        validator.clear_cache().unwrap();
        
        // Check cache is cleared
        let stats = validator.get_cache_stats().unwrap();
        assert_eq!(stats.cached_schemas, 0);
    }
}