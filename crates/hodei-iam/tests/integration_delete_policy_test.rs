//! Test de integración para HU-IAM-008: Borrar una política IAM
//!
//! Este test verifica que:
//! 1. Se puede borrar una política existente correctamente
//! 2. Se obtiene un error cuando se intenta borrar una política que no existe
//! 3. La operación es idempotente (borrar dos veces no causa error crítico)
//! 4. La API pública está correctamente expuesta

use hodei_iam::features::delete_policy::{dto::*, error::*, ports::DeletePolicyPort, DeletePolicyUseCase};
use std::sync::Arc;
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Mutex;

// Mock simple para el test de integración
#[derive(Clone)]
struct InMemoryPolicyDeleter {
    existing_policies: Arc<Mutex<HashSet<String>>>,
}

impl InMemoryPolicyDeleter {
    fn new() -> Self {
        Self {
            existing_policies: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn add_policy(&self, policy_id: String) {
        let mut policies = self.existing_policies.lock().unwrap();
        policies.insert(policy_id);
    }

    fn has_policy(&self, policy_id: &str) -> bool {
        let policies = self.existing_policies.lock().unwrap();
        policies.contains(policy_id)
    }

    fn count_policies(&self) -> usize {
        let policies = self.existing_policies.lock().unwrap();
        policies.len()
    }
}

#[async_trait]
impl DeletePolicyPort for InMemoryPolicyDeleter {
    async fn delete(&self, policy_id: &str) -> Result<(), DeletePolicyError> {
        let mut policies = self.existing_policies.lock().unwrap();
        if policies.remove(policy_id) {
            Ok(())
        } else {
            Err(DeletePolicyError::PolicyNotFound(policy_id.to_string()))
        }
    }
}

#[tokio::test]
async fn test_delete_policy_integration_success() {
    // Arrange: Crear política en el repositorio mock
    let policy_id = "production-read-only".to_string();

    let deleter = InMemoryPolicyDeleter::new();
    deleter.add_policy(policy_id.clone());

    // Verificar que existe antes de borrar
    assert!(deleter.has_policy(&policy_id), "Policy should exist before deletion");
    assert_eq!(deleter.count_policies(), 1);

    // Act: Usar la API pública del use case
    let deleter_clone = deleter.clone();
    let use_case = DeletePolicyUseCase::new(Arc::new(deleter_clone));
    let command = DeletePolicyCommand {
        policy_id: policy_id.clone(),
    };

    let result = use_case.execute(command).await;

    // Assert: Verificar que se borra correctamente
    assert!(result.is_ok(), "Expected success, got error: {:?}", result);

    // Verificar que ya no existe (usando el deleter original)
    assert!(!deleter.has_policy(&policy_id), "Policy should not exist after deletion");
    assert_eq!(deleter.count_policies(), 0);
}

#[tokio::test]
async fn test_delete_policy_integration_not_found() {
    // Arrange: Repositorio vacío
    let deleter = InMemoryPolicyDeleter::new();
    let use_case = DeletePolicyUseCase::new(Arc::new(deleter));

    let nonexistent_policy_id = "does-not-exist".to_string();
    let command = DeletePolicyCommand {
        policy_id: nonexistent_policy_id.clone(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert: Verificar error PolicyNotFound
    assert!(result.is_err(), "Expected error, got success");
    match result.unwrap_err() {
        DeletePolicyError::PolicyNotFound(id) => {
            assert_eq!(id, nonexistent_policy_id);
        }
        other => panic!("Expected PolicyNotFound, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_delete_policy_integration_multiple_policies() {
    // Arrange: Crear múltiples políticas
    let deleter = InMemoryPolicyDeleter::new();

    deleter.add_policy("policy-1".to_string());
    deleter.add_policy("policy-2".to_string());
    deleter.add_policy("policy-3".to_string());

    assert_eq!(deleter.count_policies(), 3, "Should have 3 policies initially");

    let deleter_clone = deleter.clone();
    let use_case = DeletePolicyUseCase::new(Arc::new(deleter_clone));

    // Act & Assert: Borrar policy-1
    let result1 = use_case
        .execute(DeletePolicyCommand {
            policy_id: "policy-1".to_string(),
        })
        .await;
    assert!(result1.is_ok());
    assert!(!deleter.has_policy("policy-1"));
    assert_eq!(deleter.count_policies(), 2);

    // Act & Assert: Borrar policy-3
    let result3 = use_case
        .execute(DeletePolicyCommand {
            policy_id: "policy-3".to_string(),
        })
        .await;
    assert!(result3.is_ok());
    assert!(!deleter.has_policy("policy-3"));
    assert_eq!(deleter.count_policies(), 1);

    // Verify policy-2 still exists
    assert!(deleter.has_policy("policy-2"), "policy-2 should still exist");
}

#[tokio::test]
async fn test_delete_policy_integration_invalid_id() {
    // Arrange
    let deleter = InMemoryPolicyDeleter::new();
    let use_case = DeletePolicyUseCase::new(Arc::new(deleter));

    // Act: Intentar borrar con ID vacío
    let result = use_case
        .execute(DeletePolicyCommand {
            policy_id: "".to_string(),
        })
        .await;

    // Assert: Debe fallar con InvalidPolicyId
    assert!(result.is_err(), "Expected error for empty policy ID");
    match result.unwrap_err() {
        DeletePolicyError::InvalidPolicyId(_) => {}
        other => panic!("Expected InvalidPolicyId, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_delete_policy_integration_idempotency() {
    // Arrange: Crear una política
    let policy_id = "test-policy".to_string();
    let deleter = InMemoryPolicyDeleter::new();
    deleter.add_policy(policy_id.clone());

    let use_case = DeletePolicyUseCase::new(Arc::new(deleter));

    // Act: Borrar la política por primera vez
    let result1 = use_case
        .execute(DeletePolicyCommand {
            policy_id: policy_id.clone(),
        })
        .await;
    assert!(result1.is_ok(), "First deletion should succeed");

    // Act: Intentar borrar la misma política por segunda vez
    let result2 = use_case
        .execute(DeletePolicyCommand {
            policy_id: policy_id.clone(),
        })
        .await;

    // Assert: Segunda eliminación debe fallar con PolicyNotFound
    assert!(result2.is_err(), "Second deletion should fail");
    match result2.unwrap_err() {
        DeletePolicyError::PolicyNotFound(_) => {}
        other => panic!("Expected PolicyNotFound, got: {:?}", other),
    }
}
