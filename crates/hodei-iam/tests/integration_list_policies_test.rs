//! Test de integración para HU-IAM-009: Listar políticas IAM
//!
//! Este test verifica que:
//! 1. Se pueden listar todas las políticas con paginación
//! 2. La paginación funciona correctamente (limit y offset)
//! 3. Se obtiene información precisa de paginación (has_next_page, has_previous_page)
//! 4. La API pública está correctamente expuesta

use hodei_iam::features::list_policies::{
    ListPoliciesQuery, ListPoliciesUseCase, PolicySummary, InMemoryPolicyLister,
};
use kernel::Hrn;
use std::sync::Arc;

fn create_test_policies(count: usize) -> Vec<PolicySummary> {
    (0..count)
        .map(|i| PolicySummary {
            hrn: Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123456789012".to_string(),
                "Policy".to_string(),
                format!("policy-{:03}", i),
            ),
            name: format!("Policy {:03}", i),
            description: Some(format!("Test policy number {}", i)),
        })
        .collect()
}

#[tokio::test]
async fn test_list_policies_integration_empty() {
    // Arrange: Repositorio vacío
    let lister = Arc::new(InMemoryPolicyLister::new());
    let use_case = ListPoliciesUseCase::new(lister);

    // Act
    let query = ListPoliciesQuery::default();
    let result = use_case.execute(query).await;

    // Assert
    assert!(result.is_ok(), "Expected success, got error: {:?}", result);
    let response = result.unwrap();
    assert!(response.policies.is_empty());
    assert_eq!(response.page_info.total_count, 0);
    assert!(!response.page_info.has_next_page);
    assert!(!response.page_info.has_previous_page);
}

#[tokio::test]
async fn test_list_policies_integration_single_page() {
    // Arrange: 5 políticas, todas caben en una página
    let policies = create_test_policies(5);
    let lister = Arc::new(InMemoryPolicyLister::with_policies(policies));
    let use_case = ListPoliciesUseCase::new(lister);

    // Act
    let query = ListPoliciesQuery::with_limit(10);
    let result = use_case.execute(query).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.policies.len(), 5);
    assert_eq!(response.page_info.total_count, 5);
    assert_eq!(response.page_info.page_size, 5);
    assert!(!response.page_info.has_next_page);
    assert!(!response.page_info.has_previous_page);
}

#[tokio::test]
async fn test_list_policies_integration_first_page() {
    // Arrange: 50 políticas, página de 20
    let policies = create_test_policies(50);
    let lister = Arc::new(InMemoryPolicyLister::with_policies(policies));
    let use_case = ListPoliciesUseCase::new(lister);

    // Act: Primera página
    let query = ListPoliciesQuery::with_pagination(20, 0);
    let result = use_case.execute(query).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.policies.len(), 20);
    assert_eq!(response.page_info.total_count, 50);
    assert_eq!(response.page_info.current_offset, 0);
    assert!(response.page_info.has_next_page);
    assert!(!response.page_info.has_previous_page);

    // Verificar que next_offset es correcto
    assert_eq!(response.page_info.next_offset(), Some(20));
    assert_eq!(response.page_info.previous_offset(20), None);
}

#[tokio::test]
async fn test_list_policies_integration_middle_page() {
    // Arrange: 100 políticas
    let policies = create_test_policies(100);
    let lister = Arc::new(InMemoryPolicyLister::with_policies(policies));
    let use_case = ListPoliciesUseCase::new(lister);

    // Act: Página del medio (offset 40, limit 20)
    let query = ListPoliciesQuery::with_pagination(20, 40);
    let result = use_case.execute(query).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.policies.len(), 20);
    assert_eq!(response.page_info.total_count, 100);
    assert_eq!(response.page_info.current_offset, 40);
    assert!(response.page_info.has_next_page);
    assert!(response.page_info.has_previous_page);

    // Verificar offsets de navegación
    assert_eq!(response.page_info.next_offset(), Some(60));
    assert_eq!(response.page_info.previous_offset(20), Some(20));
}

#[tokio::test]
async fn test_list_policies_integration_last_page() {
    // Arrange: 95 políticas, páginas de 20
    let policies = create_test_policies(95);
    let lister = Arc::new(InMemoryPolicyLister::with_policies(policies));
    let use_case = ListPoliciesUseCase::new(lister);

    // Act: Última página (offset 80, debería devolver 15 elementos)
    let query = ListPoliciesQuery::with_pagination(20, 80);
    let result = use_case.execute(query).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.policies.len(), 15); // Solo quedan 15
    assert_eq!(response.page_info.total_count, 95);
    assert_eq!(response.page_info.current_offset, 80);
    assert!(!response.page_info.has_next_page);
    assert!(response.page_info.has_previous_page);

    assert_eq!(response.page_info.next_offset(), None);
    assert_eq!(response.page_info.previous_offset(20), Some(60));
}

#[tokio::test]
async fn test_list_policies_integration_offset_beyond_total() {
    // Arrange: 10 políticas
    let policies = create_test_policies(10);
    let lister = Arc::new(InMemoryPolicyLister::with_policies(policies));
    let use_case = ListPoliciesUseCase::new(lister);

    // Act: Offset más allá del total
    let query = ListPoliciesQuery::with_pagination(10, 100);
    let result = use_case.execute(query).await;

    // Assert: Debe devolver página vacía pero sin error
    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.policies.is_empty());
    assert_eq!(response.page_info.total_count, 10);
    assert!(!response.page_info.has_next_page);
    assert!(response.page_info.has_previous_page); // Hay páginas anteriores
}

#[tokio::test]
async fn test_list_policies_integration_default_limit() {
    // Arrange: 60 políticas
    let policies = create_test_policies(60);
    let lister = Arc::new(InMemoryPolicyLister::with_policies(policies));
    let use_case = ListPoliciesUseCase::new(lister);

    // Act: Sin especificar limit (debería usar default=50)
    let query = ListPoliciesQuery::new();
    let result = use_case.execute(query).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.policies.len(), 50); // Default limit
    assert_eq!(response.page_info.total_count, 60);
    assert!(response.page_info.has_next_page);
}

#[tokio::test]
async fn test_list_policies_integration_max_limit() {
    // Arrange: 150 políticas
    let policies = create_test_policies(150);
    let lister = Arc::new(InMemoryPolicyLister::with_policies(policies));
    let use_case = ListPoliciesUseCase::new(lister);

    // Act: Solicitar más de 100 (debería limitarse a 100)
    let query = ListPoliciesQuery::with_limit(200);
    let result = use_case.execute(query).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.policies.len(), 100); // Capped at 100
    assert_eq!(response.page_info.total_count, 150);
    assert!(response.page_info.has_next_page);
}

#[tokio::test]
async fn test_list_policies_integration_invalid_limit_zero() {
    // Arrange
    let lister = Arc::new(InMemoryPolicyLister::new());
    let use_case = ListPoliciesUseCase::new(lister);

    // Act: Limit = 0 debería fallar
    let query = ListPoliciesQuery {
        limit: Some(0),
        offset: None,
    };
    let result = use_case.execute(query).await;

    // Assert: Debe retornar error de validación
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("must be greater than 0"));
}

#[tokio::test]
async fn test_list_policies_integration_navigation_flow() {
    // Arrange: 55 políticas
    let policies = create_test_policies(55);
    let lister = Arc::new(InMemoryPolicyLister::with_policies(policies));
    let use_case = ListPoliciesUseCase::new(lister);

    // Act & Assert: Navegar por todas las páginas
    let page_size = 20;
    let mut current_offset = 0;
    let mut total_retrieved = 0;

    // Primera página
    let query = ListPoliciesQuery::with_pagination(page_size, current_offset);
    let response = use_case.execute(query).await.unwrap();
    assert_eq!(response.policies.len(), 20);
    assert!(response.page_info.has_next_page);
    total_retrieved += response.policies.len();

    // Segunda página
    current_offset = response.page_info.next_offset().unwrap();
    let query = ListPoliciesQuery::with_pagination(page_size, current_offset);
    let response = use_case.execute(query).await.unwrap();
    assert_eq!(response.policies.len(), 20);
    assert!(response.page_info.has_next_page);
    total_retrieved += response.policies.len();

    // Tercera página (última)
    current_offset = response.page_info.next_offset().unwrap();
    let query = ListPoliciesQuery::with_pagination(page_size, current_offset);
    let response = use_case.execute(query).await.unwrap();
    assert_eq!(response.policies.len(), 15);
    assert!(!response.page_info.has_next_page);
    total_retrieved += response.policies.len();

    // Verificar que obtuvimos todas las políticas
    assert_eq!(total_retrieved, 55);
}

#[tokio::test]
async fn test_list_policies_integration_consistent_ordering() {
    // Arrange: Políticas con nombres que no están en orden alfabético
    let mut policies = vec![
        PolicySummary {
            hrn: Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "Policy".to_string(),
                "zebra".to_string(),
            ),
            name: "Zebra Policy".to_string(),
            description: None,
        },
        PolicySummary {
            hrn: Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "Policy".to_string(),
                "alpha".to_string(),
            ),
            name: "Alpha Policy".to_string(),
            description: None,
        },
        PolicySummary {
            hrn: Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123".to_string(),
                "Policy".to_string(),
                "beta".to_string(),
            ),
            name: "Beta Policy".to_string(),
            description: None,
        },
    ];

    let lister = Arc::new(InMemoryPolicyLister::with_policies(policies));
    let use_case = ListPoliciesUseCase::new(lister);

    // Act
    let query = ListPoliciesQuery::default();
    let response = use_case.execute(query).await.unwrap();

    // Assert: Deben estar ordenadas alfabéticamente por nombre
    assert_eq!(response.policies[0].name, "Alpha Policy");
    assert_eq!(response.policies[1].name, "Beta Policy");
    assert_eq!(response.policies[2].name, "Zebra Policy");
}

#[tokio::test]
async fn test_list_policies_integration_policy_summary_fields() {
    // Arrange
    let policy = PolicySummary {
        hrn: Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "Policy".to_string(),
            "production-read-only".to_string(),
        ),
        name: "ProductionReadOnly".to_string(),
        description: Some("Read-only access to production resources".to_string()),
    };

    let lister = Arc::new(InMemoryPolicyLister::with_policies(vec![policy.clone()]));
    let use_case = ListPoliciesUseCase::new(lister);

    // Act
    let query = ListPoliciesQuery::default();
    let response = use_case.execute(query).await.unwrap();

    // Assert: Verificar que todos los campos están presentes
    assert_eq!(response.policies.len(), 1);
    let retrieved = &response.policies[0];
    assert_eq!(retrieved.hrn, policy.hrn);
    assert_eq!(retrieved.name, "ProductionReadOnly");
    assert_eq!(
        retrieved.description,
        Some("Read-only access to production resources".to_string())
    );
}

