// crates/repository/src/features/delete_repository/use_case_test.rs

use std::sync::Arc;
use shared::hrn::{OrganizationId, UserId, RepositoryId};
use shared::enums::Ecosystem;
use time::OffsetDateTime;

use crate::domain::repository::{RepositoryType, DeploymentPolicy};
use crate::domain::RepositoryError;
use super::dto::{DeleteRepositoryCommand, DeleteRepositoryResponse};
use super::use_case::DeleteRepositoryUseCase;
use super::di::test_adapter::{
    MockRepositoryDeleterPort, MockRepositoryDeleteAuthorizationPort,
    MockArtifactDeleterPort, MockRepositoryDeleteEventPublisherPort
};

#[tokio::test]
async fn test_delete_repository_success() {
    // Arrange
    let deleter_port = Arc::new(MockRepositoryDeleterPort::new());
    let authorization_port = Arc::new(MockRepositoryDeleteAuthorizationPort::new());
    let artifact_deleter_port = Arc::new(MockArtifactDeleterPort::new());
    let event_publisher_port = Arc::new(MockRepositoryDeleteEventPublisherPort::new());

    let use_case = DeleteRepositoryUseCase::new(
        deleter_port.clone(),
        authorization_port.clone(),
        artifact_deleter_port.clone(),
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
    *deleter_port.mock_repository.lock().unwrap() = Some(mock_repository.clone());

    let command = DeleteRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        force: false,
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
    assert!(response.success);
    assert!(response.message.contains("successfully deleted"));

    // Verificar que se publicó el evento
    let published_events = event_publisher_port.published_events.lock().unwrap();
    assert!(!published_events.is_empty());
    let (repo_id, user, artifact_count, total_size) = &published_events[0];
    assert_eq!(repo_id.as_str(), repository_id.as_str());
    assert_eq!(user.as_str(), UserId::new_system_user().as_str());
    assert_eq!(*artifact_count, 0); // Vacío
    assert_eq!(*total_size, 0);
}

#[tokio::test]
async fn test_delete_repository_not_found() {
    // Arrange
    let deleter_port = Arc::new(MockRepositoryDeleterPort::new());
    *deleter_port.should_return_none.lock().unwrap() = true;

    let authorization_port = Arc::new(MockRepositoryDeleteAuthorizationPort::new());
    let artifact_deleter_port = Arc::new(MockArtifactDeleterPort::new());
    let event_publisher_port = Arc::new(MockRepositoryDeleteEventPublisherPort::new());

    let use_case = DeleteRepositoryUseCase::new(
        deleter_port,
        authorization_port,
        artifact_deleter_port,
        event_publisher_port,
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "nonexistent-repo").unwrap();

    let command = DeleteRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        force: false,
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
async fn test_delete_repository_unauthorized() {
    // Arrange
    let deleter_port = Arc::new(MockRepositoryDeleterPort::new());
    let authorization_port = Arc::new(MockRepositoryDeleteAuthorizationPort::new());
    *authorization_port.should_authorize.lock().unwrap() = false; // No autorizar

    let artifact_deleter_port = Arc::new(MockArtifactDeleterPort::new());
    let event_publisher_port = Arc::new(MockRepositoryDeleteEventPublisherPort::new());

    let use_case = DeleteRepositoryUseCase::new(
        deleter_port,
        authorization_port,
        artifact_deleter_port,
        event_publisher_port,
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "test-repo").unwrap();

    let command = DeleteRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        force: false,
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
async fn test_delete_repository_database_error() {
    // Arrange
    let deleter_port = Arc::new(MockRepositoryDeleterPort::new());
    *deleter_port.should_fail.lock().unwrap() = true; // Simular error de base de datos

    let authorization_port = Arc::new(MockRepositoryDeleteAuthorizationPort::new());
    let artifact_deleter_port = Arc::new(MockArtifactDeleterPort::new());
    let event_publisher_port = Arc::new(MockRepositoryDeleteEventPublisherPort::new());

    let use_case = DeleteRepositoryUseCase::new(
        deleter_port,
        authorization_port,
        artifact_deleter_port,
        event_publisher_port,
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "test-repo").unwrap();

    let command = DeleteRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        force: false,
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
async fn test_delete_repository_invalid_hrn() {
    // Arrange
    let deleter_port = Arc::new(MockRepositoryDeleterPort::new());
    let authorization_port = Arc::new(MockRepositoryDeleteAuthorizationPort::new());
    let artifact_deleter_port = Arc::new(MockArtifactDeleterPort::new());
    let event_publisher_port = Arc::new(MockRepositoryDeleteEventPublisherPort::new());

    let use_case = DeleteRepositoryUseCase::new(
        deleter_port,
        authorization_port,
        artifact_deleter_port,
        event_publisher_port,
    );

    let user_id = UserId::new_system_user();

    let command = DeleteRepositoryCommand {
        repository_hrn: "invalid-hrn-format".to_string(),
        force: false,
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
async fn test_delete_repository_not_empty_without_force() {
    // Arrange
    let deleter_port = Arc::new(MockRepositoryDeleterPort::new());
    let authorization_port = Arc::new(MockRepositoryDeleteAuthorizationPort::new());
    let artifact_deleter_port = Arc::new(MockArtifactDeleterPort::with_artifacts(5)); // 5 artefactos
    let event_publisher_port = Arc::new(MockRepositoryDeleteEventPublisherPort::new());

    let use_case = DeleteRepositoryUseCase::new(
        deleter_port.clone(),
        authorization_port.clone(),
        artifact_deleter_port.clone(),
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
    *deleter_port.mock_repository.lock().unwrap() = Some(mock_repository.clone());

    let command = DeleteRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        force: false, // Sin forzar
    };

    // Act
    let result = use_case.execute(command, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::RepositoryNotEmpty(message) => {
            assert!(message.contains("not empty"));
            assert!(message.contains("5 artifacts"));
        },
        _ => panic!("Expected RepositoryNotEmpty error"),
    }
}

#[tokio::test]
async fn test_delete_repository_not_empty_with_force() {
    // Arrange
    let deleter_port = Arc::new(MockRepositoryDeleterPort::new());
    let authorization_port = Arc::new(MockRepositoryDeleteAuthorizationPort::new());
    let artifact_deleter_port = Arc::new(MockArtifactDeleterPort::with_artifacts(5)); // 5 artefactos
    let event_publisher_port = Arc::new(MockRepositoryDeleteEventPublisherPort::new());

    let use_case = DeleteRepositoryUseCase::new(
        deleter_port.clone(),
        authorization_port.clone(),
        artifact_deleter_port.clone(),
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
    *deleter_port.mock_repository.lock().unwrap() = Some(mock_repository.clone());

    let command = DeleteRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        force: true, // Forzar eliminación
    };

    // Act
    let result = use_case.execute(command, user_id).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.hrn, repository_id.as_str());
    assert_eq!(response.name, "test-repo");
    assert_eq!(response.final_stats.artifact_count, 5); // 5 artefactos eliminados

    // Verificar que se publicó el evento con el conteo correcto
    let published_events = event_publisher_port.published_events.lock().unwrap();
    assert!(!published_events.is_empty());
    let (repo_id, user, artifact_count, total_size) = &published_events[0];
    assert_eq!(repo_id.as_str(), repository_id.as_str());
    assert_eq!(user.as_str(), UserId::new_system_user().as_str());
    assert_eq!(*artifact_count, 5);
    assert_eq!(*total_size, 0);
}

#[tokio::test]
async fn test_delete_repository_artifact_deletion_error() {
    // Arrange
    let deleter_port = Arc::new(MockRepositoryDeleterPort::new());
    let authorization_port = Arc::new(MockRepositoryDeleteAuthorizationPort::new());
    let artifact_deleter_port = Arc::new(MockArtifactDeleterPort::with_artifacts(3));
    *artifact_deleter_port.should_fail.lock().unwrap() = true; // Error al eliminar artefactos

    let event_publisher_port = Arc::new(MockRepositoryDeleteEventPublisherPort::new());

    let use_case = DeleteRepositoryUseCase::new(
        deleter_port.clone(),
        authorization_port.clone(),
        artifact_deleter_port.clone(),
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
    *deleter_port.mock_repository.lock().unwrap() = Some(mock_repository.clone());

    let command = DeleteRepositoryCommand {
        repository_hrn: repository_id.as_str().to_string(),
        force: true, // Forzar para permitir eliminación con artefactos
    };

    // Act
    let result = use_case.execute(command, user_id).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RepositoryError::DatabaseError(message) => {
            assert_eq!(message, "Mock artifact deletion error");
        },
        _ => panic!("Expected DatabaseError"),
    }
}