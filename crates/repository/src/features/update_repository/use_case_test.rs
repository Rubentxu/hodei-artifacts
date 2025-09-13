// crates/repository/src/features/update_repository/use_case_test.rs

use std::sync::Arc;
use shared::hrn::{OrganizationId, UserId, RepositoryId, Hrn};
use shared::enums::Ecosystem;
use crate::domain::repository::{Repository, RepositoryType, DeploymentPolicy, HostedConfig, RepositoryConfig};
use crate::domain::RepositoryError;
use super::dto::{UpdateRepositoryCommand, RepositoryConfigUpdateDto, HostedConfigUpdateDto, DeploymentPolicyUpdateDto};
use super::use_case::UpdateRepositoryUseCase;
use super::di::UpdateRepositoryDIContainer;
use crate::infrastructure::mongodb_adapter::MongoDbRepositoryAdapter;
use crate::features::update_repository::ports::*;
use tokio;


#[tokio::test]
async fn test_update_repository_success() {
    // Arrange
    let container = UpdateRepositoryDIContainer::for_testing().await;
    let use_case = container.endpoint.use_case;
    let db = container.endpoint.use_case.repository_updater_port;

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

    let command = UpdateRepositoryCommand {
        repository_hrn: repository_id.to_string(),
        config: Some(RepositoryConfigUpdateDto::Hosted(HostedConfigUpdateDto {
            deployment_policy: Some(DeploymentPolicyUpdateDto::BlockSnapshots),
        })),
        storage_backend_hrn: Some(Hrn::new("hrn:hodei:storage:::{}:new-default").unwrap()),
    };

    // Act
    let result = use_case.execute(command, user_id).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.hrn, repository_id.to_string());
    assert_eq!(response.name, "test-repo");
    assert_eq!(response.repo_type, RepositoryType::Hosted);
    assert_eq!(response.format, Ecosystem::Maven);
    assert_eq!(response.storage_backend_hrn, Some(Hrn::new("hrn:hodei:storage:::{}:new-default").unwrap()));
}

// Add other tests similarly, using the DI container to get the use case
// and mocking the necessary ports.