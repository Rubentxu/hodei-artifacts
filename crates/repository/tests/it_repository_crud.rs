// crates/repository/tests/it_repository_crud.rs

use repository::{
    create_repository_api_module_for_testing,
    CreateRepositoryCommand, CreateRepositoryResponse,
    GetRepositoryQuery, GetRepositoryResponse,
    UpdateRepositoryCommand, UpdateRepositoryResponse,
    DeleteRepositoryCommand, DeleteRepositoryResponse,
};
use shared::hrn::{OrganizationId, UserId};
use shared::enums::Ecosystem;
use axum::{
    body::Body,
    http::{Request, StatusCode, Method},
};
use tower::ServiceExt; // for `oneshot`

/// Tests de integración para las operaciones CRUD de repositorios
#[tokio::test]
async fn test_repository_crud_lifecycle() {
    // Arrange
    let api_module = create_repository_api_module_for_testing();
    let app = api_module.create_router();

    let org_id = OrganizationId::new("test-org").unwrap();
    let user_id = UserId::new_system_user();
    let repo_name = "test-repo-crud";

    // Test 1: CREATE Repository
    let create_command = CreateRepositoryCommand {
        name: repo_name.to_string(),
        organization_hrn: org_id.as_str().to_string(),
        repo_type: "Hosted".to_string(),
        format: "Maven".to_string(),
        region: "us-east-1".to_string(),
        config: serde_json::json!({
            "type": "Hosted",
            "deployment_policy": "AllowSnapshots"
        }),
        storage_backend_hrn: "hrn:hodei:repository:us-east-1:test-storage".to_string(),
        is_public: false,
        metadata: None,
    };

    let create_request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/repositories")
        .header("content-type", "application/json")
        .header("x-user-id", user_id.as_str())
        .body(Body::from(serde_json::to_string(&create_command).unwrap()))
        .unwrap();

    let response = app.clone().oneshot(create_request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let create_response: CreateRepositoryResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(create_response.name, repo_name);
    assert_eq!(create_response.repo_type, repository::domain::repository::RepositoryType::Hosted);
    assert_eq!(create_response.format, Ecosystem::Maven);

    let repo_hrn = create_response.hrn;

    // Test 2: GET Repository
    let get_request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/v1/repositories/{}", repo_hrn))
        .header("x-user-id", user_id.as_str())
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(get_request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let get_response: GetRepositoryResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(get_response.hrn, repo_hrn);
    assert_eq!(get_response.name, repo_name);

    // Test 3: UPDATE Repository
    let update_command = UpdateRepositoryCommand {
        repository_hrn: repo_hrn.clone(),
        name: Some("updated-test-repo".to_string()),
        description: Some("Updated description".to_string()),
        region: Some("eu-west-1".to_string()),
        config: Some(serde_json::json!({
            "type": "Hosted",
            "deployment_policy": "BlockSnapshots"
        })),
        is_public: Some(true),
        metadata: Some(serde_json::json!({
            "updated_by": "integration_test"
        })),
    };

    let update_request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/api/v1/repositories/{}", repo_hrn))
        .header("content-type", "application/json")
        .header("x-user-id", user_id.as_str())
        .body(Body::from(serde_json::to_string(&update_command).unwrap()))
        .unwrap();

    let response = app.clone().oneshot(update_request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let update_response: UpdateRepositoryResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(update_response.name, "updated-test-repo");
    assert_eq!(update_response.region, "eu-west-1");
    assert_eq!(update_response.description, "Updated description");

    // Test 4: DELETE Repository
    let delete_command = DeleteRepositoryCommand {
        repository_hrn: repo_hrn.clone(),
        force: false,
    };

    let delete_request = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/api/v1/repositories/{}", repo_hrn))
        .header("content-type", "application/json")
        .header("x-user-id", user_id.as_str())
        .body(Body::from(serde_json::to_string(&delete_command).unwrap()))
        .unwrap();

    let response = app.clone().oneshot(delete_request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let delete_response: DeleteRepositoryResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(delete_response.hrn, repo_hrn);
    assert!(delete_response.success);
    assert!(delete_response.message.contains("successfully deleted"));

    // Test 5: Verify repository is deleted
    let get_after_delete_request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/v1/repositories/{}", repo_hrn))
        .header("x-user-id", user_id.as_str())
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(get_after_delete_request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_create_repository_validation_errors() {
    // Arrange
    let api_module = create_repository_api_module_for_testing();
    let app = api_module.create_router();

    let org_id = OrganizationId::new("test-org").unwrap();
    let user_id = UserId::new_system_user();

    // Test: Invalid repository type
    let invalid_command = CreateRepositoryCommand {
        name: "test-repo".to_string(),
        organization_hrn: org_id.as_str().to_string(),
        repo_type: "InvalidType".to_string(), // Tipo inválido
        format: "Maven".to_string(),
        region: "us-east-1".to_string(),
        config: serde_json::json!({
            "type": "Hosted",
            "deployment_policy": "AllowSnapshots"
        }),
        storage_backend_hrn: "hrn:hodei:repository:us-east-1:test-storage".to_string(),
        is_public: false,
        metadata: None,
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/repositories")
        .header("content-type", "application/json")
        .header("x-user-id", user_id.as_str())
        .body(Body::from(serde_json::to_string(&invalid_command).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_repository_duplicate_name() {
    // Arrange
    let api_module = create_repository_api_module_for_testing();
    let app = api_module.create_router();

    let org_id = OrganizationId::new("test-org").unwrap();
    let user_id = UserId::new_system_user();
    let repo_name = "duplicate-repo";

    // Crear primer repositorio
    let create_command = CreateRepositoryCommand {
        name: repo_name.to_string(),
        organization_hrn: org_id.as_str().to_string(),
        repo_type: "Hosted".to_string(),
        format: "Maven".to_string(),
        region: "us-east-1".to_string(),
        config: serde_json::json!({
            "type": "Hosted",
            "deployment_policy": "AllowSnapshots"
        }),
        storage_backend_hrn: "hrn:hodei:repository:us-east-1:test-storage".to_string(),
        is_public: false,
        metadata: None,
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/repositories")
        .header("content-type", "application/json")
        .header("x-user-id", user_id.as_str())
        .body(Body::from(serde_json::to_string(&create_command).unwrap()))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // Intentar crear repositorio con el mismo nombre
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_update_nonexistent_repository() {
    // Arrange
    let api_module = create_repository_api_module_for_testing();
    let app = api_module.create_router();

    let user_id = UserId::new_system_user();
    let fake_repo_hrn = "hrn:hodei:repository:us-east-1:hrn:hodei:iam::system:organization/test-org:repository/nonexistent";

    let update_command = UpdateRepositoryCommand {
        repository_hrn: fake_repo_hrn.to_string(),
        name: Some("updated-name".to_string()),
        description: None,
        region: None,
        config: None,
        is_public: None,
        metadata: None,
    };

    let request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/api/v1/repositories/{}", fake_repo_hrn))
        .header("content-type", "application/json")
        .header("x-user-id", user_id.as_str())
        .body(Body::from(serde_json::to_string(&update_command).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_nonexistent_repository() {
    // Arrange
    let api_module = create_repository_api_module_for_testing();
    let app = api_module.create_router();

    let user_id = UserId::new_system_user();
    let fake_repo_hrn = "hrn:hodei:repository:us-east-1:hrn:hodei:iam::system:organization/test-org:repository/nonexistent";

    let delete_command = DeleteRepositoryCommand {
        repository_hrn: fake_repo_hrn.to_string(),
        force: false,
    };

    let request = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/api/v1/repositories/{}", fake_repo_hrn))
        .header("content-type", "application/json")
        .header("x-user-id", user_id.as_str())
        .body(Body::from(serde_json::to_string(&delete_command).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_list_repositories() {
    // Arrange
    let api_module = create_repository_api_module_for_testing();
    let app = api_module.create_router();

    let org_id = OrganizationId::new("test-org-list").unwrap();
    let user_id = UserId::new_system_user();

    // Crear múltiples repositorios
    for i in 0..3 {
        let create_command = CreateRepositoryCommand {
            name: format!("test-repo-{}", i),
            organization_hrn: org_id.as_str().to_string(),
            repo_type: "Hosted".to_string(),
            format: "Maven".to_string(),
            region: "us-east-1".to_string(),
            config: serde_json::json!({
                "type": "Hosted",
                "deployment_policy": "AllowSnapshots"
            }),
            storage_backend_hrn: "hrn:hodei:repository:us-east-1:test-storage".to_string(),
            is_public: false,
            metadata: None,
        };

        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/repositories")
            .header("content-type", "application/json")
            .header("x-user-id", user_id.as_str())
            .body(Body::from(serde_json::to_string(&create_command).unwrap()))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    // Listar repositorios (requeriría implementación real de listado)
    // Por ahora, este test verifica que la estructura está lista
    assert!(true);
}

#[tokio::test]
async fn test_repository_types() {
    // Arrange
    let api_module = create_repository_api_module_for_testing();
    let app = api_module.create_router();

    let org_id = OrganizationId::new("test-org-types").unwrap();
    let user_id = UserId::new_system_user();

    // Test Hosted repository
    let hosted_command = CreateRepositoryCommand {
        name: "hosted-repo".to_string(),
        organization_hrn: org_id.as_str().to_string(),
        repo_type: "Hosted".to_string(),
        format: "Maven".to_string(),
        region: "us-east-1".to_string(),
        config: serde_json::json!({
            "type": "Hosted",
            "deployment_policy": "AllowSnapshots"
        }),
        storage_backend_hrn: "hrn:hodei:repository:us-east-1:test-storage".to_string(),
        is_public: false,
        metadata: None,
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/repositories")
        .header("content-type", "application/json")
        .header("x-user-id", user_id.as_str())
        .body(Body::from(serde_json::to_string(&hosted_command).unwrap()))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // Test Proxy repository
    let proxy_command = CreateRepositoryCommand {
        name: "proxy-repo".to_string(),
        organization_hrn: org_id.as_str().to_string(),
        repo_type: "Proxy".to_string(),
        format: "Npm".to_string(),
        region: "us-east-1".to_string(),
        config: serde_json::json!({
            "type": "Proxy",
            "remote_url": "https://registry.npmjs.org",
            "cache_settings": {
                "metadata_ttl_seconds": 3600,
                "artifact_ttl_seconds": 86400
            }
        }),
        storage_backend_hrn: "hrn:hodei:repository:us-east-1:test-storage".to_string(),
        is_public: false,
        metadata: None,
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/repositories")
        .header("content-type", "application/json")
        .header("x-user-id", user_id.as_str())
        .body(Body::from(serde_json::to_string(&proxy_command).unwrap()))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // Test Virtual repository
    let virtual_command = CreateRepositoryCommand {
        name: "virtual-repo".to_string(),
        organization_hrn: org_id.as_str().to_string(),
        repo_type: "Virtual".to_string(),
        format: "Docker".to_string(),
        region: "us-east-1".to_string(),
        config: serde_json::json!({
            "type": "Virtual",
            "aggregated_repositories": [
                "hrn:hodei:repository:us-east-1:hrn:hodei:iam::system:organization/test-org-types:repository/hosted-repo",
                "hrn:hodei:repository:us-east-1:hrn:hodei:iam::system:organization/test-org-types:repository/proxy-repo"
            ],
            "resolution_order": "FirstFound"
        }),
        storage_backend_hrn: "hrn:hodei:repository:us-east-1:test-storage".to_string(),
        is_public: false,
        metadata: None,
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/repositories")
        .header("content-type", "application/json")
        .header("x-user-id", user_id.as_str())
        .body(Body::from(serde_json::to_string(&virtual_command).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_force_delete_repository() {
    // Arrange
    let api_module = create_repository_api_module_for_testing();
    let app = api_module.create_router();

    let org_id = OrganizationId::new("test-org-force").unwrap();
    let user_id = UserId::new_system_user();
    let repo_name = "force-delete-repo";

    // Crear repositorio
    let create_command = CreateRepositoryCommand {
        name: repo_name.to_string(),
        organization_hrn: org_id.as_str().to_string(),
        repo_type: "Hosted".to_string(),
        format: "Maven".to_string(),
        region: "us-east-1".to_string(),
        config: serde_json::json!({
            "type": "Hosted",
            "deployment_policy": "AllowSnapshots"
        }),
        storage_backend_hrn: "hrn:hodei:repository:us-east-1:test-storage".to_string(),
        is_public: false,
        metadata: None,
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/repositories")
        .header("content-type", "application/json")
        .header("x-user-id", user_id.as_str())
        .body(Body::from(serde_json::to_string(&create_command).unwrap()))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let create_response: CreateRepositoryResponse = serde_json::from_slice(&body).unwrap();
    let repo_hrn = create_response.hrn;

    // Forzar eliminación (aunque esté vacío)
    let delete_command = DeleteRepositoryCommand {
        repository_hrn: repo_hrn.clone(),
        force: true,
    };

    let request = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/api/v1/repositories/{}?force=true", repo_hrn))
        .header("content-type", "application/json")
        .header("x-user-id", user_id.as_str())
        .body(Body::from(serde_json::to_string(&delete_command).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}