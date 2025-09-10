// crates/repository/src/features/update_repository/use_case_test.rs

use std::sync::Arc;
use shared::hrn::{OrganizationId, UserId, RepositoryId};
use shared::enums::Ecosystem;
use time::OffsetDateTime;

use crate::domain::repository::{RepositoryType, DeploymentPolicy};
use crate::domain::RepositoryError;
use super::dto::{UpdateRepositoryCommand, RepositoryConfigUpdateDto, HostedConfigUpdateDto, DeploymentPolicyUpdateDto};
use super::use_case::UpdateRepositoryUseCase;
use super::di::test_adapter::{
    MockRepositoryUpdaterPort, MockRepositoryUpdateAuthorizationPort,
    MockRepositoryConfigValidatorPort, MockRepositoryUpdateEventPublisherPort
};

#[tokio::test]
async fn test_update_repository_success() {
    // Arrange
    let updater_port = Arc::new(MockRepositoryUpdaterPort::new());
    let authorization_port = Arc::new(MockRepositoryUpdateAuthorizationPort::new());
    let config_validator_port = Arc::new(MockRepositoryConfigValidatorPort::new());
    let event_publisher_port = Arc::new(MockRepositoryUpdateEventPublisherPort::new());

    let use_case = UpdateRepositoryUseCase::new(
        updater_port.clone(),
        authorization_port.clone(),
        config_validator_port.clone(),
        event_publisher_port.clone(),
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "test-repo").unwrap();

    // Crear un repositorio mock
    let mock_repository = crate::domain::repository::Repository {
        hrn: repository_id.clone(),
        organization_hrn: organization_id,
        name: "test-repo".to_string(),
        region: "us-east-1".to_string(),
        repo_type: RepositoryType::Hosted,
        format: Ecosystem::Maven,
        config: crate::domain::repository::RepositoryConfig::Hosted(
            crate::domain::repository::HostedConfig {
                deployment_policy: DeploymentPolicy::AllowSnapshots,
            }
        ),
        storage_backend_hrn: "hrn:hodei:repository:us-east-1:hrn:hodei:iam::system:organization/test-org:storage-backend/default".to_string(),
        lifecycle: shared::lifecycle::Lifecycle::new(shared::hrn::Hrn::new("hrn:hodei:iam::system:user/system").unwrap()),
    };

    // Configurar el mock para devolver el repositorio
    *updater_port.mock_repository.lock().unwrap() = Some(mock_repository.clone());

    let command = UpdateRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        description: Some("Updated description".to_string()),
        config: Some(RepositoryConfigUpdateDto::Hosted(HostedConfigUpdateDto {
            deployment_policy: Some(DeploymentPolicyUpdateDto::BlockSnapshots),
        })),
        storage_backend_hrn: Some("hrn:hodei:repository:us-east-1:new-storage".to_string()),
        is_public: Some(true),
        metadata: Some({
            let mut map = std::collections::HashMap::new();
            map.insert("key".to_string(), "value".to_string());
            map
        }),
    };

    // Act
    let result = use_case.execute(command, user_id).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.hrn, repository_id.as_str());
    assert_eq!(response.name, "test-repo");
    assert_eq!(response.repo_type, RepositoryType::Hosted);
    assert_eq!(response.format, Ecosystem::Maven);
    assert_eq!(response.storage_backend_hrn, Some("hrn:hodei:repository:us-east-1:new-storage".to_string()));

    // Verificar que se publicó el evento
    let published_events = event_publisher_port.published_events.lock().unwrap();
    assert!(!published_events.is_empty());
    let (repo_id, user, changes) = &published_events[0];
    assert_eq!(repo_id.as_str(), repository_id.as_str());
    assert_eq!(user.as_str(), UserId::new_system_user().as_str());
    assert!(!changes.is_empty());
}

#[tokio::test]
async fn test_update_repository_not_found() {
    // Arrange
    let updater_port = Arc::new(MockRepositoryUpdaterPort::new());
    *updater_port.should_return_none.lock().unwrap() = true;

    let authorization_port = Arc::new(MockRepositoryUpdateAuthorizationPort::new());
    let config_validator_port = Arc::new(MockRepositoryConfigValidatorPort::new());
    let event_publisher_port = Arc::new(MockRepositoryUpdateEventPublisherPort::new());

    let use_case = UpdateRepositoryUseCase::new(
        updater_port,
        authorization_port,
        config_validator_port,
        event_publisher_port,
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "nonexistent-repo").unwrap();

    let command = UpdateRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        description: None,
        config: None,
        storage_backend_hrn: None,
        is_public: None,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::RepositoryNotFound(repo_id) => {
            assert_eq!(repo_id, repository_id.as_str());
        },
        _ => panic!("Expected RepositoryNotFound error"),
    }
}

#[tokio::test]
async fn test_update_repository_unauthorized() {
    // Arrange
    let updater_port = Arc::new(MockRepositoryUpdaterPort::new());
    let authorization_port = Arc::new(MockRepositoryUpdateAuthorizationPort::new());
    *authorization_port.should_authorize.lock().unwrap() = false; // No autorizar

    let config_validator_port = Arc::new(MockRepositoryConfigValidatorPort::new());
    let event_publisher_port = Arc::new(MockRepositoryUpdateEventPublisherPort::new());

    let use_case = UpdateRepositoryUseCase::new(
        updater_port,
        authorization_port,
        config_validator_port,
        event_publisher_port,
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "test-repo").unwrap();

    let command = UpdateRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        description: None,
        config: None,
        storage_backend_hrn: None,
        is_public: None,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::Unauthorized(message) => {
            assert!(message.contains("don't have permission"));
        },
        _ => panic!("Expected Unauthorized error"),
    }
}

#[tokio::test]
async fn test_update_repository_database_error() {
    // Arrange
    let updater_port = Arc::new(MockRepositoryUpdaterPort::new());
    *updater_port.should_fail.lock().unwrap() = true; // Simular error de base de datos

    let authorization_port = Arc::new(MockRepositoryUpdateAuthorizationPort::new());
    let config_validator_port = Arc::new(MockRepositoryConfigValidatorPort::new());
    let event_publisher_port = Arc::new(MockRepositoryUpdateEventPublisherPort::new());

    let use_case = UpdateRepositoryUseCase::new(
        updater_port,
        authorization_port,
        config_validator_port,
        event_publisher_port,
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "test-repo").unwrap();

    let command = UpdateRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        description: None,
        config: None,
        storage_backend_hrn: None,
        is_public: None,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::DatabaseError(message) => {
            assert_eq!(message, "Mock database error");
        },
        _ => panic!("Expected DatabaseError"),
    }
}

#[tokio::test]
async fn test_update_repository_invalid_hrn() {
    // Arrange
    let updater_port = Arc::new(MockRepositoryUpdaterPort::new());
    let authorization_port = Arc::new(MockRepositoryUpdateAuthorizationPort::new());
    let config_validator_port = Arc::new(MockRepositoryConfigValidatorPort::new());
    let event_publisher_port = Arc::new(MockRepositoryUpdateEventPublisherPort::new());

    let use_case = UpdateRepositoryUseCase::new(
        updater_port,
        authorization_port,
        config_validator_port,
        event_publisher_port,
    );

    let user_id = UserId::new_system_user();

    let command = UpdateRepositoryCommand {
        repository_hrn: "invalid-hrn-format".to_string(),
        description: None,
        config: None,
        storage_backend_hrn: None,
        is_public: None,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::InvalidRepositoryName(message) => {
            assert!(message.contains("Invalid repository HRN"));
        },
        _ => panic!("Expected InvalidRepositoryName error"),
    }
}

#[tokio::test]
async fn test_update_repository_type_mismatch() {
    // Arrange
    let updater_port = Arc::new(MockRepositoryUpdaterPort::new());
    let authorization_port = Arc::new(MockRepositoryUpdateAuthorizationPort::new());
    let config_validator_port = Arc::new(MockRepositoryConfigValidatorPort::new());
    *config_validator_port.should_fail_type_consistency.lock().unwrap() = true; // Forzar error de tipo

    let event_publisher_port = Arc::new(MockRepositoryUpdateEventPublisherPort::new());

    let use_case = UpdateRepositoryUseCase::new(
        updater_port,
        authorization_port,
        config_validator_port,
        event_publisher_port,
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "test-repo").unwrap();

    let command = UpdateRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        description: None,
        config: Some(RepositoryConfigUpdateDto::Hosted(HostedConfigUpdateDto {
            deployment_policy: Some(DeploymentPolicyUpdateDto::BlockSnapshots),
        })),
        storage_backend_hrn: None,
        is_public: None,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::RepositoryTypeMismatch { expected, actual } => {
            assert_eq!(expected, "Hosted");
            assert_eq!(actual, "Proxy");
        },
        _ => panic!("Expected RepositoryTypeMismatch error"),
    }
}

#[tokio::test]
async fn test_update_repository_config_validation_error() {
    // Arrange
    let updater_port = Arc::new(MockRepositoryUpdaterPort::new());
    let authorization_port = Arc::new(MockRepositoryUpdateAuthorizationPort::new());
    let config_validator_port = Arc::new(MockRepositoryConfigValidatorPort::new());
    *config_validator_port.should_fail.lock().unwrap() = true; // Forzar error de validación

    let event_publisher_port = Arc::new(MockRepositoryUpdateEventPublisherPort::new());

    let use_case = UpdateRepositoryUseCase::new(
        updater_port,
        authorization_port,
        config_validator_port,
        event_publisher_port,
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "test-repo").unwrap();

    let command = UpdateRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        description: None,
        config: Some(RepositoryConfigUpdateDto::Hosted(HostedConfigUpdateDto {
            deployment_policy: Some(DeploymentPolicyUpdateDto::BlockSnapshots),
        })),
        storage_backend_hrn: None,
        is_public: None,
        metadata: None,
    };

    // Act
    let result = use_case.execute(command, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::InvalidConfiguration(message) => {
            assert_eq!(message, "Mock validation error");
        },
        _ => panic!("Expected InvalidConfiguration error"),
    }
}

#[tokio::test]
async fn test_update_repository_partial_update() {
    // Arrange
    let updater_port = Arc::new(MockRepositoryUpdaterPort::new());
    let authorization_port = Arc::new(MockRepositoryUpdateAuthorizationPort::new());
    let config_validator_port = Arc::new(MockRepositoryConfigValidatorPort::new());
    let event_publisher_port = Arc::new(MockRepositoryUpdateEventPublisherPort::new());

    let use_case = UpdateRepositoryUseCase::new(
        updater_port.clone(),
        authorization_port.clone(),
        config_validator_port.clone(),
        event_publisher_port.clone(),
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "test-repo").unwrap();

    // Crear un repositorio mock
    let mock_repository = crate::domain::repository::Repository {
        hrn: repository_id.clone(),
        organization_hrn: organization_id,
        name: "test-repo".to_string(),
        region: "us-east-1".to_string(),
        repo_type: RepositoryType::Hosted,
        format: Ecosystem::Maven,
        config: crate::domain::repository::RepositoryConfig::Hosted(
            crate::domain::repository::HostedConfig {
                deployment_policy: DeploymentPolicy::AllowSnapshots,
            }
        ),
        storage_backend_hrn: "hrn:hodei:repository:us-east-1:hrn:hodei:iam::system:organization/test-org:storage-backend/default".to_string(),
        lifecycle: shared::lifecycle::Lifecycle::new(shared::hrn::Hrn::new("hrn:hodei:iam::system:user/system").unwrap()),
    };

    // Configurar el mock para devolver el repositorio
    *updater_port.mock_repository.lock().unwrap() = Some(mock_repository.clone());

    let command = UpdateRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        description: None, // No actualizar descripción
        config: None, // No actualizar configuración
        storage_backend_hrn: Some("hrn:hodei:repository:us-east-1:new-storage".to_string()), // Solo actualizar storage
        is_public: None, // No actualizar visibilidad
        metadata: None, // No actualizar metadatos
    };

    // Act
    let result = use_case.execute(command, user_id).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.hrn, repository_id.as_str());
    assert_eq!(response.name, "test-repo");
    assert_eq!(response.storage_backend_hrn, Some("hrn:hodei:repository:us-east-1:new-storage".to_string()));
}