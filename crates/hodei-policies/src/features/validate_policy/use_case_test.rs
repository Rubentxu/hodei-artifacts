use super::dto::ValidatePolicyCommand;
use super::use_case::ValidatePolicyUseCase;
use crate::features::build_schema::error::BuildSchemaError;
use crate::features::load_schema::ports::SchemaStoragePort;
use async_trait::async_trait;

// Mock storage for testing
#[derive(Clone)]
struct MockSchemaStorage;

#[async_trait]
impl SchemaStoragePort for MockSchemaStorage {
    async fn save_schema(
        &self,
        _schema_json: String,
        _version: Option<String>,
    ) -> Result<String, BuildSchemaError> {
        Ok("schema_1".to_string())
    }

    async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
        Ok(None)
    }

    async fn get_schema_by_version(
        &self,
        _version: &str,
    ) -> Result<Option<String>, BuildSchemaError> {
        Ok(None)
    }

    async fn delete_schema(&self, _schema_id: &str) -> Result<bool, BuildSchemaError> {
        Ok(true)
    }

    async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn test_valid_policy_returns_is_valid_true() {
    let use_case = ValidatePolicyUseCase::<MockSchemaStorage>::new();
    let command = ValidatePolicyCommand {
        content: "permit(principal, action, resource);".to_string(),
    };
    let result = use_case.execute(command).await.unwrap();
    assert!(result.is_valid);
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn test_invalid_policy_returns_is_valid_false_with_errors() {
    let use_case = ValidatePolicyUseCase::<MockSchemaStorage>::new();
    let command = ValidatePolicyCommand {
        content: "permit(principal, action);".to_string(),
    }; // Sintaxis incorrecta
    let result = use_case.execute(command).await.unwrap();
    assert!(!result.is_valid);
    assert!(!result.errors.is_empty());
    // Cedar parsing error messages can vary, just check that there's an error
    assert!(!result.errors[0].is_empty());
}

#[tokio::test]
async fn test_empty_policy_is_invalid() {
    let use_case = ValidatePolicyUseCase::<MockSchemaStorage>::new();
    let command = ValidatePolicyCommand {
        content: "   ".to_string(),
    };
    let result = use_case.execute(command).await.unwrap();
    assert!(!result.is_valid);
    assert_eq!(result.errors[0], "Policy content cannot be empty");
}
