// crates/repository/src/features/create_repository/use_case_test.rs

use std::sync::Arc;
use shared::hrn::{OrganizationId, UserId};
use shared::enums::Ecosystem;
use time::OffsetDateTime;

use crate::domain::repository::{RepositoryType, DeploymentPolicy};
use crate::domain::RepositoryError;
use super::dto::{CreateRepositoryCommand, RepositoryConfigDto, HostedConfigDto, DeploymentPolicyDto};
use super::use_case::CreateRepositoryUseCase;
use super::di::test_adapter::{
    MockOrganizationExistsPort, MockRepositoryExistsPort, MockRepositoryCreatorPort,
    MockStorageBackendExistsPort, MockEventPublisherPort, MockNameValidatorPort,
    MockConfigValidatorPort
};

#[tokio::test]
async fn test_create_repository_success() {
    // Arrange
    let organization_exists_port = Arc::new(MockOrganizationExistsPort::new());
    let repository_exists_port = Arc::new(MockRepositoryExistsPort::new());
    let repository_creator_port = Arc::new(MockRepositoryCreatorPort::new());
    let storage_backend_exists_port = Arc::new(MockStorageBackendExistsPort::new());
    let event_publisher_port = Arc::new(MockEventPublisherPort::new());
    let name_validator_port = Arc::new(MockNameValidatorPort::new());
    let config_validator_port = Arc::new(MockConfigValidatorPort::new());

    let use_case = CreateRepositoryUseCase::new(
        organization_exists_port.clone(),
        repository_exists_port.clone(),
        repository_creator_port.clone(),
        storage_backend_exists_port.clone(),
        event_publisher_port.clone(),
        name_validator_port.clone(),
        config_validator_port.clone(),
    );

    let organization_id = OrganizationId::new("test-org").unwrap();
    let user_id = UserId::new_system_user();

    let command = CreateRepositoryCommand {
        name: "test-repo".to_string(),
        description: Some("Test repository".to_string()),
        repo_type: RepositoryType::Hosted,
        format: Ecosystem::Maven,
        config: RepositoryConfigDto::Hosted(HostedConfigDto {
            deployment_policy: DeploymentPolicyDto::AllowSnapshots,
        }),
        storage_backend_hrn: Some("hrn:hodei:repository:us-east-1:hrn:hodei:iam::system:organization/test-org:storage-backend/default".to_string()),
        is_public: false,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, organization_id, user_id).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.name, "test-repo");
    assert_eq!(response.repo_type, RepositoryType::Hosted);
    assert_eq!(response.format, Ecosystem::Maven);

    // Verificar que el repositorio fue creado
    let created_repos = repository_creator_port.created_repositories.lock().unwrap();
    assert_eq!(created_repos.len(), 1);
    assert!(created_repos[0].contains("test-repo"));

    // Verificar que el evento fue publicado
    let published_events = event_publisher_port.published_events.lock().unwrap();
    assert_eq!(published_events.len(), 1);
    assert!(published_events[0].contains("test-repo"));
}

#[tokio::test]
async fn test_create_repository_organization_not_found() {
    // Arrange
    let organization_exists_port = Arc::new(MockOrganizationExistsPort::new());
    *organization_exists_port.should_exist.lock().unwrap() = false;

    let repository_exists_port = Arc::new(MockRepositoryExistsPort::new());
    let repository_creator_port = Arc::new(MockRepositoryCreatorPort::new());
    let storage_backend_exists_port = Arc::new(MockStorageBackendExistsPort::new());
    let event_publisher_port = Arc::new(MockEventPublisherPort::new());
    let name_validator_port = Arc::new(MockNameValidatorPort::new());
    let config_validator_port = Arc::new(MockConfigValidatorPort::new());

    let use_case = CreateRepositoryUseCase::new(
        organization_exists_port,
        repository_exists_port,
        repository_creator_port,
        storage_backend_exists_port,
        event_publisher_port,
        name_validator_port,
        config_validator_port,
    );

    let organization_id = OrganizationId::new("nonexistent-org").unwrap();
    let user_id = UserId::new_system_user();

    let command = CreateRepositoryCommand {
        name: "test-repo".to_string(),
        description: None,
        repo_type: RepositoryType::Hosted,
        format: Ecosystem::Maven,
        config: RepositoryConfigDto::Hosted(HostedConfigDto {
            deployment_policy: DeploymentPolicyDto::AllowSnapshots,
        }),
        storage_backend_hrn: None,
        is_public: false,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, organization_id, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::OrganizationNotFound(org_id) => {
            assert_eq!(org_id, "hrn:hodei:iam::system:organization/nonexistent-org");
        },
        _ => panic!("Expected OrganizationNotFound error"),
    }
}

#[tokio::test]
async fn test_create_repository_already_exists() {
    // Arrange
    let organization_exists_port = Arc::new(MockOrganizationExistsPort::new());
    let repository_exists_port = Arc::new(MockRepositoryExistsPort::new());
    *repository_exists_port.should_exist.lock().unwrap() = true; // Repository already exists

    let repository_creator_port = Arc::new(MockRepositoryCreatorPort::new());
    let storage_backend_exists_port = Arc::new(MockStorageBackendExistsPort::new());
    let event_publisher_port = Arc::new(MockEventPublisherPort::new());
    let name_validator_port = Arc::new(MockNameValidatorPort::new());
    let config_validator_port = Arc::new(MockConfigValidatorPort::new());

    let use_case = CreateRepositoryUseCase::new(
        organization_exists_port,
        repository_exists_port,
        repository_creator_port,
        storage_backend_exists_port,
        event_publisher_port,
        name_validator_port,
        config_validator_port,
    );

    let organization_id = OrganizationId::new("test-org").unwrap();
    let user_id = UserId::new_system_user();

    let command = CreateRepositoryCommand {
        name: "existing-repo".to_string(),
        description: None,
        repo_type: RepositoryType::Hosted,
        format: Ecosystem::Maven,
        config: RepositoryConfigDto::Hosted(HostedConfigDto {
            deployment_policy: DeploymentPolicyDto::AllowSnapshots,
        }),
        storage_backend_hrn: None,
        is_public: false,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, organization_id, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::RepositoryAlreadyExists(name) => {
            assert_eq!(name, "existing-repo");
        },
        _ => panic!("Expected RepositoryAlreadyExists error"),
    }
}

#[tokio::test]
async fn test_create_repository_invalid_name() {
    // Arrange
    let organization_exists_port = Arc::new(MockOrganizationExistsPort::new());
    let repository_exists_port = Arc::new(MockRepositoryExistsPort::new());
    let repository_creator_port = Arc::new(MockRepositoryCreatorPort::new());
    let storage_backend_exists_port = Arc::new(MockStorageBackendExistsPort::new());
    let event_publisher_port = Arc::new(MockEventPublisherPort::new());
    let name_validator_port = Arc::new(MockNameValidatorPort::new());
    *name_validator_port.should_fail.lock().unwrap() = true;
    *name_validator_port.failure_message.lock().unwrap() = "Invalid repository name".to_string();

    let config_validator_port = Arc::new(MockConfigValidatorPort::new());

    let use_case = CreateRepositoryUseCase::new(
        organization_exists_port,
        repository_exists_port,
        repository_creator_port,
        storage_backend_exists_port,
        event_publisher_port,
        name_validator_port,
        config_validator_port,
    );

    let organization_id = OrganizationId::new("test-org").unwrap();
    let user_id = UserId::new_system_user();

    let command = CreateRepositoryCommand {
        name: "invalid-name".to_string(),
        description: None,
        repo_type: RepositoryType::Hosted,
        format: Ecosystem::Maven,
        config: RepositoryConfigDto::Hosted(HostedConfigDto {
            deployment_policy: DeploymentPolicyDto::AllowSnapshots,
        }),
        storage_backend_hrn: None,
        is_public: false,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, organization_id, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::InvalidRepositoryName(message) => {
            assert_eq!(message, "Invalid repository name");
        },
        _ => panic!("Expected InvalidRepositoryName error"),
    }
}

#[tokio::test]
async fn test_create_repository_invalid_config() {
    // Arrange
    let organization_exists_port = Arc::new(MockOrganizationExistsPort::new());
    let repository_exists_port = Arc::new(MockRepositoryExistsPort::new());
    let repository_creator_port = Arc::new(MockRepositoryCreatorPort::new());
    let storage_backend_exists_port = Arc::new(MockStorageBackendExistsPort::new());
    let event_publisher_port = Arc::new(MockEventPublisherPort::new());
    let name_validator_port = Arc::new(MockNameValidatorPort::new());
    let config_validator_port = Arc::new(MockConfigValidatorPort::new());
    *config_validator_port.should_fail.lock().unwrap() = true;
    *config_validator_port.failure_message.lock().unwrap() = "Invalid repository configuration".to_string();

    let use_case = CreateRepositoryUseCase::new(
        organization_exists_port,
        repository_exists_port,
        repository_creator_port,
        storage_backend_exists_port,
        event_publisher_port,
        name_validator_port,
        config_validator_port,
    );

    let organization_id = OrganizationId::new("test-org").unwrap();
    let user_id = UserId::new_system_user();

    let command = CreateRepositoryCommand {
        name: "test-repo".to_string(),
        description: None,
        repo_type: RepositoryType::Hosted,
        format: Ecosystem::Maven,
        config: RepositoryConfigDto::Hosted(HostedConfigDto {
            deployment_policy: DeploymentPolicyDto::AllowSnapshots,
        }),
        storage_backend_hrn: None,
        is_public: false,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, organization_id, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::InvalidConfiguration(message) => {
            assert_eq!(message, "Invalid repository configuration");
        },
        _ => panic!("Expected InvalidConfiguration error"),
    }
}

#[tokio::test]
async fn test_create_repository_storage_backend_not_found() {
    // Arrange
    let organization_exists_port = Arc::new(MockOrganizationExistsPort::new());
    let repository_exists_port = Arc::new(MockRepositoryExistsPort::new());
    let repository_creator_port = Arc::new(MockRepositoryCreatorPort::new());
    let storage_backend_exists_port = Arc::new(MockStorageBackendExistsPort::new());
    *storage_backend_exists_port.should_exist.lock().unwrap() = false; // Storage backend doesn't exist

    let event_publisher_port = Arc::new(MockEventPublisherPort::new());
    let name_validator_port = Arc::new(MockNameValidatorPort::new());
    let config_validator_port = Arc::new(MockConfigValidatorPort::new());

    let use_case = CreateRepositoryUseCase::new(
        organization_exists_port,
        repository_exists_port,
        repository_creator_port,
        storage_backend_exists_port,
        event_publisher_port,
        name_validator_port,
        config_validator_port,
    );

    let organization_id = OrganizationId::new("test-org").unwrap();
    let user_id = UserId::new_system_user();

    let command = CreateRepositoryCommand {
        name: "test-repo".to_string(),
        description: None,
        repo_type: RepositoryType::Hosted,
        format: Ecosystem::Maven,
        config: RepositoryConfigDto::Hosted(HostedConfigDto {
            deployment_policy: DeploymentPolicyDto::AllowSnapshots,
        }),
        storage_backend_hrn: Some("hrn:hodei:repository:us-east-1:hrn:hodei:iam::system:organization/test-org:storage-backend/nonexistent".to_string()),
        is_public: false,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, organization_id, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::StorageBackendNotFound(backend) => {
            assert_eq!(backend, "hrn:hodei:repository:us-east-1:hrn:hodei:iam::system:organization/test-org:storage-backend/nonexistent");
        },
        _ => panic!("Expected StorageBackendNotFound error"),
    }
}

#[tokio::test]
async fn test_create_repository_database_error() {
    // Arrange
    let organization_exists_port = Arc::new(MockOrganizationExistsPort::new());
    let repository_exists_port = Arc::new(MockRepositoryExistsPort::new());
    let repository_creator_port = Arc::new(MockRepositoryCreatorPort::new());
    *repository_creator_port.should_fail.lock().unwrap() = true; // Simulate database error

    let storage_backend_exists_port = Arc::new(MockStorageBackendExistsPort::new());
    let event_publisher_port = Arc::new(MockEventPublisherPort::new());
    let name_validator_port = Arc::new(MockNameValidatorPort::new());
    let config_validator_port = Arc::new(MockConfigValidatorPort::new());

    let use_case = CreateRepositoryUseCase::new(
        organization_exists_port,
        repository_exists_port,
        repository_creator_port,
        storage_backend_exists_port,
        event_publisher_port,
        name_validator_port,
        config_validator_port,
    );

    let organization_id = OrganizationId::new("test-org").unwrap();
    let user_id = UserId::new_system_user();

    let command = CreateRepositoryCommand {
        name: "test-repo".to_string(),
        description: None,
        repo_type: RepositoryType::Hosted,
        format: Ecosystem::Maven,
        config: RepositoryConfigDto::Hosted(HostedConfigDto {
            deployment_policy: DeploymentPolicyDto::AllowSnapshots,
        }),
        storage_backend_hrn: None,
        is_public: false,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, organization_id, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::DatabaseError(message) => {
            assert_eq!(message, "Mock database error");
        },
        _ => panic!("Expected DatabaseError"),
    }
}