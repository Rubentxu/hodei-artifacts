// crates/repository/src/features/create_repository/use_case_test.rs

use std::sync::Arc;
use shared::hrn::{OrganizationId, UserId, Hrn};
use shared::enums::Ecosystem;
use crate::domain::repository::{RepositoryType, DeploymentPolicy, RepositoryConfig, HostedConfig};
use crate::domain::RepositoryError;
use super::dto::{CreateRepositoryCommand, RepositoryConfigDto, HostedConfigDto};
use super::use_case::CreateRepositoryUseCase;
use super::di::CreateRepositoryDIContainer;
use crate::infrastructure::mongodb_adapter::MongoDbRepositoryAdapter;
use crate::features::create_repository::ports::*;
use tokio;


#[tokio::test]
async fn test_create_repository_success() {
    // Arrange
    let (container, event_publisher_port) = CreateRepositoryDIContainer::for_testing().await;
    let use_case = container.endpoint.use_case;

    let organization_id = OrganizationId::new("test-org").unwrap();
    let user_id = UserId::new_system_user();

    let command = CreateRepositoryCommand {
        name: "test-repo".to_string(),
        repo_type: RepositoryType::Hosted,
        format: Ecosystem::Maven,
        config: RepositoryConfigDto::Hosted(HostedConfigDto {
            deployment_policy: DeploymentPolicy::AllowSnapshots,
        }),
        storage_backend_hrn: Some(Hrn::new("hrn:hodei:storage:::{}:default").unwrap()),
    };

    // Act
    let result = use_case.execute(command, organization_id, user_id).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.name, "test-repo");
    assert_eq!(response.repo_type, RepositoryType::Hosted);
    assert_eq!(response.format, Ecosystem::Maven);

    // Verificar que el evento fue publicado
    let published_events = event_publisher_port.get_published_events();
    assert_eq!(published_events.len(), 1);
    assert!(published_events[0].contains("test-repo"));
}

// Add other tests similarly, using the DI container to get the use case
// and mocking the necessary ports.