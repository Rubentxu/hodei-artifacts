// crates/repository/src/features/get_repository/use_case_test.rs

use std::sync::Arc;
use shared::hrn::{OrganizationId, UserId, RepositoryId};
use shared::enums::Ecosystem;
use time::OffsetDateTime;

use crate::domain::repository::{RepositoryType, DeploymentPolicy};
use crate::domain::RepositoryError;
use super::dto::{GetRepositoryQuery, GetRepositoryResponse};
use super::use_case::GetRepositoryUseCase;
use super::di::test_adapter::{
    MockRepositoryReaderPort, MockRepositoryAuthorizationPort, MockRepositoryStatsPort
};

#[tokio::test]
async fn test_get_repository_success() {
    // Arrange
    let reader_port = Arc::new(MockRepositoryReaderPort::new());
    let authorization_port = Arc::new(MockRepositoryAuthorizationPort::new());
    let stats_port = Arc::new(MockRepositoryStatsPort::new());

    let use_case = GetRepositoryUseCase::new(
        reader_port.clone(),
        authorization_port.clone(),
        stats_port.clone(),
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
    *reader_port.mock_repository.lock().unwrap() = Some(mock_repository.clone());

    let query = GetRepositoryQuery {
        repository_hrn: repository_id.as_str().to_string(),
    };

    // Act
    let result = use_case.execute(query, user_id).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.hrn, repository_id.as_str());
    assert_eq!(response.name, "test-repo");
    assert_eq!(response.repo_type, RepositoryType::Hosted);
    assert_eq!(response.format, Ecosystem::Maven);
}

#[tokio::test]
async fn test_get_repository_not_found() {
    // Arrange
    let reader_port = Arc::new(MockRepositoryReaderPort::new());
    *reader_port.should_return_none.lock().unwrap() = true;

    let authorization_port = Arc::new(MockRepositoryAuthorizationPort::new());
    let stats_port = Arc::new(MockRepositoryStatsPort::new());

    let use_case = GetRepositoryUseCase::new(
        reader_port,
        authorization_port,
        stats_port,
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "nonexistent-repo").unwrap();

    let query = GetRepositoryQuery {
        repository_hrn: repository_id.as_str().to_string(),
    };

    // Act
    let result = use_case.execute(query, user_id).await;

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
async fn test_get_repository_unauthorized() {
    // Arrange
    let reader_port = Arc::new(MockRepositoryReaderPort::new());
    let authorization_port = Arc::new(MockRepositoryAuthorizationPort::new());
    *authorization_port.should_authorize.lock().unwrap() = false; // No autorizar

    let stats_port = Arc::new(MockRepositoryStatsPort::new());

    let use_case = GetRepositoryUseCase::new(
        reader_port,
        authorization_port,
        stats_port,
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "test-repo").unwrap();

    let query = GetRepositoryQuery {
        repository_hrn: repository_id.as_str().to_string(),
    };

    // Act
    let result = use_case.execute(query, user_id).await;

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
async fn test_get_repository_database_error() {
    // Arrange
    let reader_port = Arc::new(MockRepositoryReaderPort::new());
    *reader_port.should_fail.lock().unwrap() = true; // Simular error de base de datos

    let authorization_port = Arc::new(MockRepositoryAuthorizationPort::new());
    let stats_port = Arc::new(MockRepositoryStatsPort::new());

    let use_case = GetRepositoryUseCase::new(
        reader_port,
        authorization_port,
        stats_port,
    );

    let user_id = UserId::new_system_user();
    let organization_id = OrganizationId::new("test-org").unwrap();
    let repository_id = RepositoryId::new(&organization_id, "test-repo").unwrap();

    let query = GetRepositoryQuery {
        repository_hrn: repository_id.as_str().to_string(),
    };

    // Act
    let result = use_case.execute(query, user_id).await;

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
async fn test_get_repository_invalid_hrn() {
    // Arrange
    let reader_port = Arc::new(MockRepositoryReaderPort::new());
    let authorization_port = Arc::new(MockRepositoryAuthorizationPort::new());
    let stats_port = Arc::new(MockRepositoryStatsPort::new());

    let use_case = GetRepositoryUseCase::new(
        reader_port,
        authorization_port,
        stats_port,
    );

    let user_id = UserId::new_system_user();

    let query = GetRepositoryQuery {
        repository_hrn: "invalid-hrn-format".to_string(),
    };

    // Act
    let result = use_case.execute(query, user_id).await;

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
async fn test_get_repository_with_stats() {
    // Arrange
    let reader_port = Arc::new(MockRepositoryReaderPort::new());
    let authorization_port = Arc::new(MockRepositoryAuthorizationPort::new());
    let stats_port = Arc::new(MockRepositoryStatsPort::new());

    // Configurar estadísticas específicas
    *stats_port.artifact_count.lock().unwrap() = 25;
    *stats_port.total_size.lock().unwrap() = 1024 * 1024 * 10; // 10MB
    *stats_port.total_downloads.lock().unwrap() = 500;

    let use_case = GetRepositoryUseCase::new(
        reader_port.clone(),
        authorization_port.clone(),
        stats_port.clone(),
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
    *reader_port.mock_repository.lock().unwrap() = Some(mock_repository);

    let query = GetRepositoryQuery {
        repository_hrn: repository_id.as_str().to_string(),
    };

    // Act
    let result = use_case.execute(query, user_id).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.stats.artifact_count, 25);
    assert_eq!(response.stats.total_size_bytes, 1024 * 1024 * 10);
    assert_eq!(response.stats.total_downloads, 500);
}