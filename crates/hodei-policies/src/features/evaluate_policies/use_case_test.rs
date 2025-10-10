use super::dto::{AuthorizationRequest, Decision, EvaluatePoliciesCommand, EvaluationMode};
use super::error::EvaluatePoliciesError;
use super::use_case::EvaluatePoliciesUseCase;
use crate::features::build_schema::error::BuildSchemaError;
use crate::features::build_schema::ports::SchemaStoragePort;
use async_trait::async_trait;
use kernel::domain::policy::{HodeiPolicy, HodeiPolicySet, PolicyId};
use kernel::{AttributeValue, HodeiEntity, HodeiEntityType, Hrn};
use std::collections::HashMap;
use std::sync::Arc;

// Mock Schema Storage for testing
#[derive(Clone)]
struct MockSchemaStorage {
    should_fail: bool,
    has_schema: bool,
}

impl MockSchemaStorage {
    fn new() -> Self {
        Self {
            should_fail: false,
            has_schema: false,
        }
    }

    fn with_schema() -> Self {
        Self {
            should_fail: false,
            has_schema: true,
        }
    }

    #[allow(dead_code)]
    fn with_failure() -> Self {
        Self {
            should_fail: true,
            has_schema: false,
        }
    }
}

#[async_trait]
impl SchemaStoragePort for MockSchemaStorage {
    async fn save_schema(
        &self,
        _schema_json: String,
        _version: Option<String>,
    ) -> Result<String, BuildSchemaError> {
        if self.should_fail {
            return Err(BuildSchemaError::SchemaStorageError(
                "Mock storage error".to_string(),
            ));
        }
        Ok("schema_1".to_string())
    }

    async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
        if self.should_fail {
            return Err(BuildSchemaError::SchemaStorageError(
                "Mock storage error".to_string(),
            ));
        }
        if self.has_schema {
            Ok(Some("mock_schema_debug".to_string()))
        } else {
            Ok(None)
        }
    }

    async fn get_schema_by_version(
        &self,
        _version: &str,
    ) -> Result<Option<String>, BuildSchemaError> {
        if self.should_fail {
            return Err(BuildSchemaError::SchemaStorageError(
                "Mock storage error".to_string(),
            ));
        }
        if self.has_schema {
            Ok(Some("mock_schema_debug".to_string()))
        } else {
            Ok(None)
        }
    }

    async fn delete_schema(&self, _schema_id: &str) -> Result<bool, BuildSchemaError> {
        if self.should_fail {
            return Err(BuildSchemaError::SchemaStorageError(
                "Mock storage error".to_string(),
            ));
        }
        Ok(true)
    }

    async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
        if self.should_fail {
            return Err(BuildSchemaError::SchemaStorageError(
                "Mock storage error".to_string(),
            ));
        }
        if self.has_schema {
            Ok(vec!["v1.0.0".to_string()])
        } else {
            Ok(vec![])
        }
    }
}

// Mock entities for testing
#[derive(Debug)]
struct MockUser {
    hrn: Hrn,
    name: String,
    active: bool,
    role: String,
    department: String,
}

impl HodeiEntityType for MockUser {
    fn service_name() -> kernel::domain::ServiceName {
        kernel::domain::ServiceName::new("iam").unwrap()
    }

    fn resource_type_name() -> kernel::domain::ResourceTypeName {
        kernel::domain::ResourceTypeName::new("User").unwrap()
    }

    fn is_principal_type() -> bool {
        true
    }

    fn attributes_schema() -> Vec<(kernel::domain::AttributeName, kernel::domain::AttributeType)> {
        vec![
            (
                kernel::domain::AttributeName::new("name").unwrap(),
                kernel::domain::AttributeType::string(),
            ),
            (
                kernel::domain::AttributeName::new("active").unwrap(),
                kernel::domain::AttributeType::bool(),
            ),
            (
                kernel::domain::AttributeName::new("role").unwrap(),
                kernel::domain::AttributeType::string(),
            ),
            (
                kernel::domain::AttributeName::new("department").unwrap(),
                kernel::domain::AttributeType::string(),
            ),
        ]
    }
}

impl HodeiEntity for MockUser {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<kernel::domain::AttributeName, AttributeValue> {
        let mut attrs = HashMap::new();
        attrs.insert(
            kernel::domain::AttributeName::new("name").unwrap(),
            AttributeValue::string(&self.name),
        );
        attrs.insert(
            kernel::domain::AttributeName::new("active").unwrap(),
            AttributeValue::bool(self.active),
        );
        attrs.insert(
            kernel::domain::AttributeName::new("role").unwrap(),
            AttributeValue::string(&self.role),
        );
        attrs.insert(
            kernel::domain::AttributeName::new("department").unwrap(),
            AttributeValue::string(&self.department),
        );
        attrs
    }
}

#[derive(Debug)]
struct MockDocument {
    hrn: Hrn,
    title: String,
    classification: String,
    owner: String,
}

impl HodeiEntityType for MockDocument {
    fn service_name() -> kernel::domain::ServiceName {
        kernel::domain::ServiceName::new("storage").unwrap()
    }

    fn resource_type_name() -> kernel::domain::ResourceTypeName {
        kernel::domain::ResourceTypeName::new("Document").unwrap()
    }

    fn is_principal_type() -> bool {
        false
    }

    fn attributes_schema() -> Vec<(kernel::domain::AttributeName, kernel::domain::AttributeType)> {
        vec![
            (
                kernel::domain::AttributeName::new("title").unwrap(),
                kernel::domain::AttributeType::string(),
            ),
            (
                kernel::domain::AttributeName::new("classification").unwrap(),
                kernel::domain::AttributeType::string(),
            ),
            (
                kernel::domain::AttributeName::new("owner").unwrap(),
                kernel::domain::AttributeType::string(),
            ),
        ]
    }
}

impl HodeiEntity for MockDocument {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<kernel::domain::AttributeName, AttributeValue> {
        let mut attrs = HashMap::new();
        attrs.insert(
            kernel::domain::AttributeName::new("title").unwrap(),
            AttributeValue::string(&self.title),
        );
        attrs.insert(
            kernel::domain::AttributeName::new("classification").unwrap(),
            AttributeValue::string(&self.classification),
        );
        attrs.insert(
            kernel::domain::AttributeName::new("owner").unwrap(),
            AttributeValue::string(&self.owner),
        );
        attrs
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct MockGroup {
    hrn: Hrn,
    name: String,
    members: Vec<String>,
}

impl HodeiEntityType for MockGroup {
    fn service_name() -> kernel::domain::ServiceName {
        kernel::domain::ServiceName::new("iam").unwrap()
    }

    fn resource_type_name() -> kernel::domain::ResourceTypeName {
        kernel::domain::ResourceTypeName::new("Group").unwrap()
    }

    fn is_principal_type() -> bool {
        false
    }

    fn attributes_schema() -> Vec<(kernel::domain::AttributeName, kernel::domain::AttributeType)> {
        vec![(
            kernel::domain::AttributeName::new("name").unwrap(),
            kernel::domain::AttributeType::string(),
        )]
    }
}

impl HodeiEntity for MockGroup {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<kernel::domain::AttributeName, AttributeValue> {
        let mut attrs = HashMap::new();
        attrs.insert(
            kernel::domain::AttributeName::new("name").unwrap(),
            AttributeValue::string(&self.name),
        );
        attrs
    }
}

// Tests

#[tokio::test]
async fn test_simple_permit_allows_access() {
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let use_case = EvaluatePoliciesUseCase::new(schema_storage);

    let user = MockUser {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "hodei-test".to_string(),
            "user".to_string(),
            "alice".to_string(),
        ),
        name: "Alice".to_string(),
        active: true,
        role: "developer".to_string(),
        department: "engineering".to_string(),
    };

    let document = MockDocument {
        hrn: Hrn::new(
            "aws".to_string(),
            "storage".to_string(),
            "hodei-test".to_string(),
            "document".to_string(),
            "doc1".to_string(),
        ),
        title: "Test Document".to_string(),
        classification: "public".to_string(),
        owner: "alice".to_string(),
    };

    let policy = HodeiPolicy::new(
        PolicyId::new("policy1".to_string()),
        "permit(principal, action, resource);".to_string(),
    );
    let policy_set = HodeiPolicySet::new(vec![policy]);

    let entities: Vec<&dyn HodeiEntity> = vec![&user, &document];

    let request = AuthorizationRequest::new(&user, "read", &document);

    let command = EvaluatePoliciesCommand::new(request, &policy_set, &entities)
        .with_evaluation_mode(EvaluationMode::NoSchema);

    let result = use_case.execute(command).await.unwrap();
    assert_eq!(result.decision, Decision::Allow);
}

#[tokio::test]
async fn test_simple_forbid_denies_access() {
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let use_case = EvaluatePoliciesUseCase::new(schema_storage);

    let user = MockUser {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "hodei-test".to_string(),
            "user".to_string(),
            "bob".to_string(),
        ),
        name: "Bob".to_string(),
        active: true,
        role: "viewer".to_string(),
        department: "marketing".to_string(),
    };

    let document = MockDocument {
        hrn: Hrn::new(
            "aws".to_string(),
            "storage".to_string(),
            "hodei-test".to_string(),
            "document".to_string(),
            "doc2".to_string(),
        ),
        title: "Confidential Document".to_string(),
        classification: "confidential".to_string(),
        owner: "alice".to_string(),
    };

    let policy = HodeiPolicy::new(
        PolicyId::new("policy1".to_string()),
        "forbid(principal, action, resource);".to_string(),
    );
    let policy_set = HodeiPolicySet::new(vec![policy]);

    let entities: Vec<&dyn HodeiEntity> = vec![&user, &document];

    let request = AuthorizationRequest::new(&user, "delete", &document);

    let command = EvaluatePoliciesCommand::new(request, &policy_set, &entities).no_schema();

    let result = use_case.execute(command).await.unwrap();
    assert_eq!(result.decision, Decision::Deny);
}

#[tokio::test]
async fn test_evaluation_with_schema_best_effort_mode() {
    let schema_storage = Arc::new(MockSchemaStorage::with_schema());
    let use_case = EvaluatePoliciesUseCase::new(schema_storage);

    let user = MockUser {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "hodei-test".to_string(),
            "user".to_string(),
            "alice".to_string(),
        ),
        name: "Alice".to_string(),
        active: true,
        role: "developer".to_string(),
        department: "engineering".to_string(),
    };

    let document = MockDocument {
        hrn: Hrn::new(
            "aws".to_string(),
            "storage".to_string(),
            "hodei-test".to_string(),
            "document".to_string(),
            "doc1".to_string(),
        ),
        title: "Test Document".to_string(),
        classification: "public".to_string(),
        owner: "alice".to_string(),
    };

    let policy = HodeiPolicy::new(
        PolicyId::new("policy1".to_string()),
        "permit(principal, action, resource);".to_string(),
    );
    let policy_set = HodeiPolicySet::new(vec![policy]);

    let entities: Vec<&dyn HodeiEntity> = vec![&user, &document];

    let request = AuthorizationRequest::new(&user, "read", &document);

    let command = EvaluatePoliciesCommand::new(request, &policy_set, &entities)
        .with_evaluation_mode(EvaluationMode::BestEffortNoSchema);

    let result = use_case.execute(command).await.unwrap();
    assert_eq!(result.decision, Decision::Allow);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Using schema version"))
    );
}

#[tokio::test]
async fn test_evaluation_with_schema_not_found_best_effort_continues() {
    let schema_storage = Arc::new(MockSchemaStorage::new()); // No schema available
    let use_case = EvaluatePoliciesUseCase::new(schema_storage);

    let user = MockUser {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "hodei-test".to_string(),
            "user".to_string(),
            "alice".to_string(),
        ),
        name: "Alice".to_string(),
        active: true,
        role: "developer".to_string(),
        department: "engineering".to_string(),
    };

    let document = MockDocument {
        hrn: Hrn::new(
            "aws".to_string(),
            "storage".to_string(),
            "hodei-test".to_string(),
            "document".to_string(),
            "doc1".to_string(),
        ),
        title: "Test Document".to_string(),
        classification: "public".to_string(),
        owner: "alice".to_string(),
    };

    let policy = HodeiPolicy::new(
        PolicyId::new("policy1".to_string()),
        "permit(principal, action, resource);".to_string(),
    );
    let policy_set = HodeiPolicySet::new(vec![policy]);

    let entities: Vec<&dyn HodeiEntity> = vec![&user, &document];

    let request = AuthorizationRequest::new(&user, "read", &document);

    let command = EvaluatePoliciesCommand::new(request, &policy_set, &entities)
        .with_evaluation_mode(EvaluationMode::BestEffortNoSchema);

    // Should succeed even without schema
    let result = use_case.execute(command).await.unwrap();
    assert_eq!(result.decision, Decision::Allow);
    assert!(result.used_schema_version.is_none());
}

#[tokio::test]
async fn test_evaluation_strict_mode_fails_without_schema() {
    let schema_storage = Arc::new(MockSchemaStorage::new()); // No schema available
    let use_case = EvaluatePoliciesUseCase::new(schema_storage);

    let user = MockUser {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "hodei-test".to_string(),
            "user".to_string(),
            "alice".to_string(),
        ),
        name: "Alice".to_string(),
        active: true,
        role: "developer".to_string(),
        department: "engineering".to_string(),
    };

    let document = MockDocument {
        hrn: Hrn::new(
            "aws".to_string(),
            "storage".to_string(),
            "hodei-test".to_string(),
            "document".to_string(),
            "doc1".to_string(),
        ),
        title: "Test Document".to_string(),
        classification: "public".to_string(),
        owner: "alice".to_string(),
    };

    let policy = HodeiPolicy::new(
        PolicyId::new("policy1".to_string()),
        "permit(principal, action, resource);".to_string(),
    );
    let policy_set = HodeiPolicySet::new(vec![policy]);

    let entities: Vec<&dyn HodeiEntity> = vec![&user, &document];

    let request = AuthorizationRequest::new(&user, "read", &document);

    let command = EvaluatePoliciesCommand::new(request, &policy_set, &entities).strict_schema();

    // Should fail in strict mode without schema
    let result = use_case.execute(command).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        EvaluatePoliciesError::StrictModeSchemaRequired
    ));
}

#[tokio::test]
async fn test_evaluation_with_specific_schema_version() {
    let schema_storage = Arc::new(MockSchemaStorage::with_schema());
    let use_case = EvaluatePoliciesUseCase::new(schema_storage);

    let user = MockUser {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "hodei-test".to_string(),
            "user".to_string(),
            "alice".to_string(),
        ),
        name: "Alice".to_string(),
        active: true,
        role: "developer".to_string(),
        department: "engineering".to_string(),
    };

    let document = MockDocument {
        hrn: Hrn::new(
            "aws".to_string(),
            "storage".to_string(),
            "hodei-test".to_string(),
            "document".to_string(),
            "doc1".to_string(),
        ),
        title: "Test Document".to_string(),
        classification: "public".to_string(),
        owner: "alice".to_string(),
    };

    let policy = HodeiPolicy::new(
        PolicyId::new("policy1".to_string()),
        "permit(principal, action, resource);".to_string(),
    );
    let policy_set = HodeiPolicySet::new(vec![policy]);

    let entities: Vec<&dyn HodeiEntity> = vec![&user, &document];

    let request = AuthorizationRequest::new(&user, "read", &document);

    let command = EvaluatePoliciesCommand::new(request, &policy_set, &entities)
        .with_schema_version("v1.0.0")
        .with_evaluation_mode(EvaluationMode::BestEffortNoSchema);

    let result = use_case.execute(command).await.unwrap();
    assert_eq!(result.decision, Decision::Allow);
    assert_eq!(result.used_schema_version, Some("v1.0.0".to_string()));
}

#[tokio::test]
async fn test_policy_with_when_condition_allows() {
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let use_case = EvaluatePoliciesUseCase::new(schema_storage);

    let user = MockUser {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "hodei-test".to_string(),
            "user".to_string(),
            "alice".to_string(),
        ),
        name: "Alice".to_string(),
        active: true,
        role: "developer".to_string(),
        department: "engineering".to_string(),
    };

    let document = MockDocument {
        hrn: Hrn::new(
            "aws".to_string(),
            "storage".to_string(),
            "hodei-test".to_string(),
            "document".to_string(),
            "doc1".to_string(),
        ),
        title: "Test Document".to_string(),
        classification: "public".to_string(),
        owner: "alice".to_string(),
    };

    let policy = HodeiPolicy::new(
        PolicyId::new("policy1".to_string()),
        r#"permit(principal, action == Action::"read", resource);"#.to_string(),
    );
    let policy_set = HodeiPolicySet::new(vec![policy]);

    let entities: Vec<&dyn HodeiEntity> = vec![&user, &document];

    let request = AuthorizationRequest::new(&user, "read", &document);

    let command = EvaluatePoliciesCommand::new(request, &policy_set, &entities).no_schema();

    let result = use_case.execute(command).await.unwrap();
    assert_eq!(result.decision, Decision::Allow);
}

#[tokio::test]
async fn test_policy_with_when_condition_denies() {
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let use_case = EvaluatePoliciesUseCase::new(schema_storage);

    let user = MockUser {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "hodei-test".to_string(),
            "user".to_string(),
            "alice".to_string(),
        ),
        name: "Alice".to_string(),
        active: true,
        role: "developer".to_string(),
        department: "engineering".to_string(),
    };

    let document = MockDocument {
        hrn: Hrn::new(
            "aws".to_string(),
            "storage".to_string(),
            "hodei-test".to_string(),
            "document".to_string(),
            "doc1".to_string(),
        ),
        title: "Test Document".to_string(),
        classification: "public".to_string(),
        owner: "alice".to_string(),
    };

    let policy = HodeiPolicy::new(
        PolicyId::new("policy1".to_string()),
        r#"permit(principal, action == Action::"write", resource);"#.to_string(),
    );
    let policy_set = HodeiPolicySet::new(vec![policy]);

    let entities: Vec<&dyn HodeiEntity> = vec![&user, &document];

    let request = AuthorizationRequest::new(&user, "read", &document);

    let command = EvaluatePoliciesCommand::new(request, &policy_set, &entities).no_schema();

    let result = use_case.execute(command).await.unwrap();
    assert_eq!(result.decision, Decision::Deny);
}

#[tokio::test]
async fn test_empty_policy_set() {
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let use_case = EvaluatePoliciesUseCase::new(schema_storage);

    let user = MockUser {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "hodei-test".to_string(),
            "user".to_string(),
            "alice".to_string(),
        ),
        name: "Alice".to_string(),
        active: true,
        role: "developer".to_string(),
        department: "engineering".to_string(),
    };

    let document = MockDocument {
        hrn: Hrn::new(
            "aws".to_string(),
            "storage".to_string(),
            "hodei-test".to_string(),
            "document".to_string(),
            "doc1".to_string(),
        ),
        title: "Test Document".to_string(),
        classification: "public".to_string(),
        owner: "alice".to_string(),
    };

    let policy_set = HodeiPolicySet::new(vec![]);

    let entities: Vec<&dyn HodeiEntity> = vec![&user, &document];

    let request = AuthorizationRequest::new(&user, "read", &document);

    let command = EvaluatePoliciesCommand::new(request, &policy_set, &entities).no_schema();

    let result = use_case.execute(command).await.unwrap();
    assert_eq!(result.decision, Decision::Deny);
}

#[tokio::test]
async fn test_multiple_policies_forbid_takes_precedence() {
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let use_case = EvaluatePoliciesUseCase::new(schema_storage);

    let user = MockUser {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "hodei-test".to_string(),
            "user".to_string(),
            "alice".to_string(),
        ),
        name: "Alice".to_string(),
        active: true,
        role: "developer".to_string(),
        department: "engineering".to_string(),
    };

    let document = MockDocument {
        hrn: Hrn::new(
            "aws".to_string(),
            "storage".to_string(),
            "hodei-test".to_string(),
            "document".to_string(),
            "doc1".to_string(),
        ),
        title: "Test Document".to_string(),
        classification: "public".to_string(),
        owner: "alice".to_string(),
    };

    let permit_policy = HodeiPolicy::new(
        PolicyId::new("permit_policy".to_string()),
        "permit(principal, action, resource);".to_string(),
    );

    let forbid_policy = HodeiPolicy::new(
        PolicyId::new("forbid_policy".to_string()),
        "forbid(principal, action, resource);".to_string(),
    );

    let policy_set = HodeiPolicySet::new(vec![permit_policy, forbid_policy]);

    let entities: Vec<&dyn HodeiEntity> = vec![&user, &document];

    let request = AuthorizationRequest::new(&user, "read", &document);

    let command = EvaluatePoliciesCommand::new(request, &policy_set, &entities).no_schema();

    let result = use_case.execute(command).await.unwrap();
    assert_eq!(result.decision, Decision::Deny);
}

#[tokio::test]
async fn test_clear_cache() {
    let schema_storage = Arc::new(MockSchemaStorage::new());
    let use_case = EvaluatePoliciesUseCase::new(schema_storage);

    let result = use_case.clear_cache().await;
    assert!(result.is_ok());
}
