// crates/repository/src/features/get_repository/use_case_test.rs

use std::sync::Arc;
use shared::hrn::{OrganizationId, UserId, RepositoryId, Hrn};
use shared::enums::Ecosystem;
use crate::domain::repository::{Repository, RepositoryType, DeploymentPolicy, HostedConfig, RepositoryConfig};
use crate::domain::RepositoryError;
use super::dto::GetRepositoryQuery;
use super::use_case::GetRepositoryUseCase;
use super::di::GetRepositoryDIContainer;
use crate::infrastructure::mongodb_adapter::MongoDbRepositoryAdapter;
use crate::features::get_repository::ports::*;
use tokio;


#[tokio::test]
async fn test_get_repository_success() {
    // Arrange
    let container = GetRepositoryDIContainer::for_testing().await;
    let use_case = container.endpoint.use_case;
    let db = container.endpoint.use_case.repository_reader_port;

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id.to_string(), "test-repo").unwrap();

    // Crear un repositorio mock
    let mock_repository = Repository {
        hrn: repository_id.clone(),
        organization_hrn: organization_id,
        name: "test-repo".to_string(),
        region: "us-east-1".to_string(),
        repo_type: RepositoryType::Hosted,
        format: Ecosystem::Maven,
        config: RepositoryConfig::Hosted(
            HostedConfig {
                deployment_policy: DeploymentPolicy::AllowSnapshots,
            }
        ),
        storage_backend_hrn: Hrn::new("hrn:hodei:storage:::{}:default").unwrap(),
        lifecycle: shared::lifecycle::Lifecycle::new(Hrn::new("hrn:hodei:iam::system:user/system").unwrap()),
    };

    // Insertar el repositorio en la base de datos de prueba
    db.create_repository(&mock_repository).await.unwrap();

    let query = GetRepositoryQuery {
        repository_hrn: repository_id.to_string(),
    };

    // Act
    let result = use_case.execute(query, user_id).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.hrn, repository_id.to_string());
    assert_eq!(response.name, "test-repo");
    assert_eq!(response.repo_type, RepositoryType::Hosted);
    assert_eq!(response.format, Ecosystem::Maven);
}

// Add other tests similarly, using the DI container to get the use case
// and mocking the necessary ports.