//! Mock implementations for playground_evaluate ports
//!
//! These mocks are used for unit testing the PlaygroundEvaluateUseCase
//! without requiring actual Cedar engine integration.

use super::dto::{AttributeValue, Decision, DeterminingPolicy, PlaygroundAuthorizationRequest};
use super::error::PlaygroundEvaluateError;
use super::ports::{
    ContextConverterPort, PolicyEvaluatorPort, PolicyValidatorPort, SchemaLoaderPort,
};
use async_trait::async_trait;
use cedar_policy::Schema;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Mock schema loader for testing
///
/// This mock can be configured to return success or failure,
/// and tracks which methods were called.
pub struct MockSchemaLoader {
    /// The schema to return (if successful)
    pub schema_result: Arc<Mutex<Result<Schema, PlaygroundEvaluateError>>>,
    /// Track calls to load_schema
    pub load_calls: Arc<Mutex<Vec<(Option<String>, Option<String>)>>>,
}

impl MockSchemaLoader {
    /// Create a new mock that returns an empty schema
    pub fn new_with_success() -> Self {
        let empty_schema = Schema::from_schema_fragments(vec![]).unwrap();
        Self {
            schema_result: Arc::new(Mutex::new(Ok(empty_schema))),
            load_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a new mock that returns an error
    pub fn new_with_error(error: PlaygroundEvaluateError) -> Self {
        Self {
            schema_result: Arc::new(Mutex::new(Err(error))),
            load_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get the number of times load_schema was called
    pub fn load_call_count(&self) -> usize {
        self.load_calls.lock().unwrap().len()
    }
}

#[async_trait]
impl SchemaLoaderPort for MockSchemaLoader {
    async fn load_schema(
        &self,
        inline_schema: Option<String>,
        schema_version: Option<String>,
    ) -> Result<Schema, PlaygroundEvaluateError> {
        // Track the call
        self.load_calls
            .lock()
            .unwrap()
            .push((inline_schema, schema_version));

        // Return the configured result
        self.schema_result.lock().unwrap().clone()
    }
}

/// Mock policy validator for testing
///
/// This mock can be configured to return validation errors or success.
pub struct MockPolicyValidator {
    /// Validation errors to return (empty for success)
    pub validation_errors: Arc<Mutex<Vec<String>>>,
    /// Track calls to validate_policies
    pub validate_calls: Arc<Mutex<usize>>,
}

impl MockPolicyValidator {
    /// Create a new mock that returns success (no errors)
    pub fn new_with_success() -> Self {
        Self {
            validation_errors: Arc::new(Mutex::new(Vec::new())),
            validate_calls: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a new mock that returns validation errors
    pub fn new_with_errors(errors: Vec<String>) -> Self {
        Self {
            validation_errors: Arc::new(Mutex::new(errors)),
            validate_calls: Arc::new(Mutex::new(0)),
        }
    }

    /// Get the number of times validate_policies was called
    pub fn validate_call_count(&self) -> usize {
        *self.validate_calls.lock().unwrap()
    }
}

#[async_trait]
impl PolicyValidatorPort for MockPolicyValidator {
    async fn validate_policies(
        &self,
        _policy_texts: &[String],
        _schema: &Schema,
    ) -> Result<Vec<String>, PlaygroundEvaluateError> {
        // Track the call
        *self.validate_calls.lock().unwrap() += 1;

        // Return the configured errors
        Ok(self.validation_errors.lock().unwrap().clone())
    }
}

/// Mock policy evaluator for testing
///
/// This mock can be configured to return different decisions and determining policies.
pub struct MockPolicyEvaluator {
    /// The decision to return
    pub decision: Arc<Mutex<Decision>>,
    /// The determining policies to return
    pub determining_policies: Arc<Mutex<Vec<DeterminingPolicy>>>,
    /// Track calls to evaluate
    pub evaluate_calls: Arc<Mutex<usize>>,
}

impl MockPolicyEvaluator {
    /// Create a new mock that returns Allow with no determining policies
    pub fn new_with_allow() -> Self {
        Self {
            decision: Arc::new(Mutex::new(Decision::Allow)),
            determining_policies: Arc::new(Mutex::new(Vec::new())),
            evaluate_calls: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a new mock that returns Deny with no determining policies
    pub fn new_with_deny() -> Self {
        Self {
            decision: Arc::new(Mutex::new(Decision::Deny)),
            determining_policies: Arc::new(Mutex::new(Vec::new())),
            evaluate_calls: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a new mock with custom decision and determining policies
    pub fn new_with_result(decision: Decision, policies: Vec<DeterminingPolicy>) -> Self {
        Self {
            decision: Arc::new(Mutex::new(decision)),
            determining_policies: Arc::new(Mutex::new(policies)),
            evaluate_calls: Arc::new(Mutex::new(0)),
        }
    }

    /// Get the number of times evaluate was called
    pub fn evaluate_call_count(&self) -> usize {
        *self.evaluate_calls.lock().unwrap()
    }
}

#[async_trait]
impl PolicyEvaluatorPort for MockPolicyEvaluator {
    async fn evaluate(
        &self,
        _request: &PlaygroundAuthorizationRequest,
        _policy_texts: &[String],
        _schema: &Schema,
    ) -> Result<(Decision, Vec<DeterminingPolicy>), PlaygroundEvaluateError> {
        // Track the call
        *self.evaluate_calls.lock().unwrap() += 1;

        // Return the configured result
        let decision = *self.decision.lock().unwrap();
        let policies = self.determining_policies.lock().unwrap().clone();
        Ok((decision, policies))
    }
}

/// Mock context converter for testing
///
/// This mock always returns an empty context map (success).
pub struct MockContextConverter {
    /// Track calls to convert_context
    pub convert_calls: Arc<Mutex<usize>>,
}

impl MockContextConverter {
    /// Create a new mock context converter
    pub fn new() -> Self {
        Self {
            convert_calls: Arc::new(Mutex::new(0)),
        }
    }

    /// Get the number of times convert_context was called
    pub fn convert_call_count(&self) -> usize {
        *self.convert_calls.lock().unwrap()
    }
}

impl Default for MockContextConverter {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextConverterPort for MockContextConverter {
    fn convert_context(
        &self,
        _attributes: &HashMap<String, AttributeValue>,
    ) -> Result<HashMap<String, cedar_policy::RestrictedExpression>, PlaygroundEvaluateError> {
        // Track the call
        *self.convert_calls.lock().unwrap() += 1;

        // Return empty context (success)
        Ok(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::Hrn;

    #[tokio::test]
    async fn test_mock_schema_loader_success() {
        let loader = MockSchemaLoader::new_with_success();
        let result = loader.load_schema(Some("{}".to_string()), None).await;
        assert!(result.is_ok());
        assert_eq!(loader.load_call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_schema_loader_error() {
        let loader = MockSchemaLoader::new_with_error(PlaygroundEvaluateError::SchemaError(
            "test error".to_string(),
        ));
        let result = loader.load_schema(None, Some("v1".to_string())).await;
        assert!(result.is_err());
        assert_eq!(loader.load_call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_policy_validator_success() {
        let validator = MockPolicyValidator::new_with_success();
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let result = validator
            .validate_policies(&[String::from("test")], &schema)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
        assert_eq!(validator.validate_call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_policy_validator_with_errors() {
        let errors = vec!["error1".to_string(), "error2".to_string()];
        let validator = MockPolicyValidator::new_with_errors(errors.clone());
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let result = validator
            .validate_policies(&[String::from("test")], &schema)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), errors);
    }

    #[tokio::test]
    async fn test_mock_policy_evaluator_allow() {
        let evaluator = MockPolicyEvaluator::new_with_allow();
        let request = PlaygroundAuthorizationRequest::new(
            Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            Hrn::action("api", "read"),
            Hrn::new(
                "hodei".to_string(),
                "storage".to_string(),
                "default".to_string(),
                "Document".to_string(),
                "doc1".to_string(),
            ),
        );
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let result = evaluator
            .evaluate(&request, &[String::from("test")], &schema)
            .await;
        assert!(result.is_ok());
        let (decision, policies) = result.unwrap();
        assert_eq!(decision, Decision::Allow);
        assert_eq!(policies.len(), 0);
        assert_eq!(evaluator.evaluate_call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_policy_evaluator_deny() {
        let evaluator = MockPolicyEvaluator::new_with_deny();
        let request = PlaygroundAuthorizationRequest::new(
            Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "User".to_string(),
                "alice".to_string(),
            ),
            Hrn::action("api", "read"),
            Hrn::new(
                "hodei".to_string(),
                "storage".to_string(),
                "default".to_string(),
                "Document".to_string(),
                "doc1".to_string(),
            ),
        );
        let schema = Schema::from_schema_fragments(vec![]).unwrap();
        let result = evaluator
            .evaluate(&request, &[String::from("test")], &schema)
            .await;
        assert!(result.is_ok());
        let (decision, _) = result.unwrap();
        assert_eq!(decision, Decision::Deny);
    }

    #[test]
    fn test_mock_context_converter() {
        let converter = MockContextConverter::new();
        let mut attrs = HashMap::new();
        attrs.insert(
            "test".to_string(),
            AttributeValue::String("value".to_string()),
        );
        let result = converter.convert_context(&attrs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
        assert_eq!(converter.convert_call_count(), 1);
    }
}
