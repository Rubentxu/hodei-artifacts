//! Test de integración para HU-IAM-006: Leer una política IAM
//!
//! Este test verifica que:
//! 1. Se puede obtener una política existente por su HRN
//! 2. Se obtiene un error cuando la política no existe
//! 3. La API pública está correctamente expuesta

use hodei_iam::features::get_policy::{dto::*, error::*, ports::PolicyReader, GetPolicyUseCase};
use kernel::Hrn;
use std::sync::Arc;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

// Mock simple para el test de integración
struct InMemoryPolicyReader {
    policies: Arc<Mutex<HashMap<String, PolicyView>>>,
}

impl InMemoryPolicyReader {
    fn new() -> Self {
        Self {
            policies: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn add_policy(&self, policy: PolicyView) {
        let mut policies = self.policies.lock().unwrap();
        policies.insert(policy.hrn.to_string(), policy);
    }
}

#[async_trait]
impl PolicyReader for InMemoryPolicyReader {
    async fn get_by_hrn(&self, hrn: &Hrn) -> Result<PolicyView, GetPolicyError> {
        let policies = self.policies.lock().unwrap();
        policies
            .get(&hrn.to_string())
            .cloned()
            .ok_or_else(|| GetPolicyError::PolicyNotFound(hrn.to_string()))
    }
}

#[tokio::test]
async fn test_get_policy_integration_success() {
    // Arrange: Crear política en el repositorio mock
    let hrn = Hrn::new(
        "aws".to_string(),
        "iam".to_string(),
        "123456789012".to_string(),
        "Policy".to_string(),
        "production-read-only".to_string(),
    );

    let policy = PolicyView {
        hrn: hrn.clone(),
        name: "ProductionReadOnly".to_string(),
        content: "permit(principal, action == Action::\"Read\", resource);".to_string(),
        description: Some("Read-only access to production resources".to_string()),
    };

    let reader = InMemoryPolicyReader::new();
    reader.add_policy(policy.clone());

    // Act: Usar la API pública del use case
    let use_case = GetPolicyUseCase::new(Arc::new(reader));
    let query = GetPolicyQuery {
        policy_hrn: hrn.clone(),
    };

    let result = use_case.execute(query).await;

    // Assert: Verificar que se obtiene la política correctamente
    assert!(result.is_ok(), "Expected success, got error: {:?}", result);
    let retrieved_policy = result.unwrap();
    assert_eq!(retrieved_policy.hrn, hrn);
    assert_eq!(retrieved_policy.name, "ProductionReadOnly");
    assert_eq!(retrieved_policy.content, "permit(principal, action == Action::\"Read\", resource);");
    assert_eq!(retrieved_policy.description, Some("Read-only access to production resources".to_string()));
}

#[tokio::test]
async fn test_get_policy_integration_not_found() {
    // Arrange: Repositorio vacío
    let reader = InMemoryPolicyReader::new();
    let use_case = GetPolicyUseCase::new(Arc::new(reader));

    let nonexistent_hrn = Hrn::new(
        "aws".to_string(),
        "iam".to_string(),
        "123456789012".to_string(),
        "Policy".to_string(),
        "does-not-exist".to_string(),
    );

    let query = GetPolicyQuery {
        policy_hrn: nonexistent_hrn.clone(),
    };

    // Act
    let result = use_case.execute(query).await;

    // Assert: Verificar error PolicyNotFound
    assert!(result.is_err(), "Expected error, got success");
    match result.unwrap_err() {
        GetPolicyError::PolicyNotFound(hrn_str) => {
            assert!(hrn_str.contains("does-not-exist"));
        }
        other => panic!("Expected PolicyNotFound, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_get_policy_integration_multiple_policies() {
    // Arrange: Crear múltiples políticas
    let reader = InMemoryPolicyReader::new();

    let policy1 = PolicyView {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "Policy".to_string(),
            "policy-1".to_string(),
        ),
        name: "Policy 1".to_string(),
        content: "permit(principal, action, resource);".to_string(),
        description: None,
    };

    let policy2 = PolicyView {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "Policy".to_string(),
            "policy-2".to_string(),
        ),
        name: "Policy 2".to_string(),
        content: "forbid(principal, action == Action::\"Delete\", resource);".to_string(),
        description: Some("Forbid delete actions".to_string()),
    };

    reader.add_policy(policy1.clone());
    reader.add_policy(policy2.clone());

    let use_case = GetPolicyUseCase::new(Arc::new(reader));

    // Act & Assert: Obtener policy-1
    let result1 = use_case
        .execute(GetPolicyQuery {
            policy_hrn: policy1.hrn.clone(),
        })
        .await;
    assert!(result1.is_ok());
    assert_eq!(result1.unwrap().name, "Policy 1");

    // Act & Assert: Obtener policy-2
    let result2 = use_case
        .execute(GetPolicyQuery {
            policy_hrn: policy2.hrn.clone(),
        })
        .await;
    assert!(result2.is_ok());
    assert_eq!(result2.unwrap().name, "Policy 2");
}
