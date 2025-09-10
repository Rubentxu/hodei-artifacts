//! Example of integrating the distribution module with the main server
//! 
//! This example demonstrates how to:
//! 1. Initialize the distribution service
//! 2. Create API routes for different package formats
//! 3. Handle requests from Maven, npm, and Docker clients

use std::sync::Arc;
use axum::{
    routing::{get, put, head},
    Router, Server,
    extract::{Path, State},
    http::{StatusCode, HeaderMap},
    response::Response,
};
use distribution::{
    DistributionIntegration, DistributionApiState,
    features::api::{MavenApi, NpmApi, DockerApi},
};

/// Example state for the distribution API
#[derive(Clone)]
struct AppState {
    distribution_state: DistributionApiState,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the distribution integration
    let integration = DistributionIntegration::new().await?;
    
    // Create the API state
    let api_state = integration.create_api_state();
    
    // Create application state
    let app_state = AppState {
        distribution_state: api_state,
    };
    
    // Build the router with all format-specific routes
    let app = create_distribution_router(app_state);
    
    // Start the server
    let addr = "0.0.0.0:8080".parse()?;
    println!("Distribution server running on {}", addr);
    
    Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    
    Ok(())
}

/// Create the main distribution router with all format endpoints
fn create_distribution_router(state: AppState) -> Router {
    Router::new()
        // Maven endpoints
        .nest("/maven", create_maven_router(state.clone()))
        // npm endpoints  
        .nest("/npm", create_npm_router(state.clone()))
        // Docker Registry V2 endpoints
        .nest("/v2", create_docker_router(state))
        .with_state(state)
}

/// Maven-specific routes
fn create_maven_router(state: AppState) -> Router<AppState> {
    Router::new()
        // GET artifact (download)
        .route("/*path", get(handle_maven_get))
        // PUT artifact (upload)
        .route("/*path", put(handle_maven_put))
        // HEAD artifact (check existence)
        .route("/*path", head(handle_maven_head))
        // GET metadata
        .route("/*path/maven-metadata.xml", get(handle_maven_metadata))
}

/// npm-specific routes
fn create_npm_router(state: AppState) -> Router<AppState> {
    Router::new()
        // GET package (download)
        .route("/:package", get(handle_npm_get_package))
        // PUT package (publish)
        .route("/:package", put(handle_npm_put_package))
        // GET package metadata
        .route("/:package/package.json", get(handle_npm_package_json))
        // GET scoped package
        .route("/:scope/:package", get(handle_npm_get_scoped))
        // PUT scoped package
        .route("/:scope/:package", put(handle_npm_put_scoped))
}

/// Docker Registry V2 routes
fn create_docker_router(state: AppState) -> Router<AppState> {
    Router::new()
        // API version check
        .route("/", get(handle_docker_version))
        // Catalog (repository list)
        .route("/_catalog", get(handle_docker_catalog))
        // Manifest operations
        .route("/:name/manifests/:reference", get(handle_docker_get_manifest))
        .route("/:name/manifests/:reference", put(handle_docker_put_manifest))
        .route("/:name/manifests/:reference", head(handle_docker_head_manifest))
        // Blob operations
        .route("/:name/blobs/:digest", get(handle_docker_get_blob))
        .route("/:name/blobs/:digest", head(handle_docker_head_blob))
        .route("/:name/blobs/uploads/", post(handle_docker_start_upload))
        .route("/:name/blobs/uploads/:uuid", put(handle_docker_complete_upload))
        // Tags list
        .route("/:name/tags/list", get(handle_docker_tags_list))
}

// Maven handlers
async fn handle_maven_get(
    State(state): State<AppState>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    // Implementation would use the Maven API from distribution crate
    // This is a placeholder showing the pattern
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body("Maven artifact download".into())
        .unwrap())
}

async fn handle_maven_put(
    State(state): State<AppState>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    // Implementation would use the Maven API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .body("Maven artifact uploaded".into())
        .unwrap())
}

async fn handle_maven_head(
    State(state): State<AppState>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    // Implementation would use the Maven API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body("Maven artifact exists".into())
        .unwrap())
}

async fn handle_maven_metadata(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<Response, StatusCode> {
    // Implementation would use the Maven metadata generation from distribution crate
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/xml")
        .body("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<metadata>...</metadata>".into())
        .unwrap())
}

// npm handlers
async fn handle_npm_get_package(
    State(state): State<AppState>,
    Path(package): Path<String>,
) -> Result<Response, StatusCode> {
    // Implementation would use the npm API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(r#"{"name":"example","version":"1.0.0"}"#.into())
        .unwrap())
}

async fn handle_npm_put_package(
    State(state): State<AppState>,
    Path(package): Path<String>,
) -> Result<Response, StatusCode> {
    // Implementation would use the npm API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .body("npm package published".into())
        .unwrap())
}

async fn handle_npm_package_json(
    State(state): State<AppState>,
    Path(package): Path<String>,
) -> Result<Response, StatusCode> {
    // Implementation would use the npm metadata generation from distribution crate
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(r#"{"name":"example","dist-tags":{"latest":"1.0.0"}}"#.into())
        .unwrap())
}

async fn handle_npm_get_scoped(
    State(state): State<AppState>,
    Path((scope, package)): Path<(String, String)>,
) -> Result<Response, StatusCode> {
    // Implementation would use the npm API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(r#"{"name":"@scope/package","version":"1.0.0"}"#.into())
        .unwrap())
}

async fn handle_npm_put_scoped(
    State(state): State<AppState>,
    Path((scope, package)): Path<(String, String)>,
) -> Result<Response, StatusCode> {
    // Implementation would use the npm API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .body("Scoped npm package published".into())
        .unwrap())
}

// Docker handlers
async fn handle_docker_version() -> Result<Response, StatusCode> {
    // Docker Registry V2 API version check
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Docker-Distribution-Api-Version", "registry/2.0")
        .body("".into())
        .unwrap())
}

async fn handle_docker_catalog(
    State(state): State<AppState>,
) -> Result<Response, StatusCode> {
    // Implementation would use the Docker API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(r#"{"repositories":["library/nginx","library/redis"]}"#.into())
        .unwrap())
}

async fn handle_docker_get_manifest(
    State(state): State<AppState>,
    Path((name, reference)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    // Implementation would use the Docker API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/vnd.docker.distribution.manifest.v2+json")
        .header("Docker-Content-Digest", "sha256:1234567890abcdef")
        .body(r#"{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json"}"#.into())
        .unwrap())
}

async fn handle_docker_put_manifest(
    State(state): State<AppState>,
    Path((name, reference)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    // Implementation would use the Docker API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .header("Docker-Content-Digest", "sha256:1234567890abcdef")
        .body("".into())
        .unwrap())
}

async fn handle_docker_head_manifest(
    State(state): State<AppState>,
    Path((name, reference)): Path<(String, String)>,
) -> Result<Response, StatusCode> {
    // Implementation would use the Docker API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Docker-Content-Digest", "sha256:1234567890abcdef")
        .body("".into())
        .unwrap())
}

async fn handle_docker_get_blob(
    State(state): State<AppState>,
    Path((name, digest)): Path<(String, String)>,
) -> Result<Response, StatusCode> {
    // Implementation would use the Docker API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/octet-stream")
        .header("Docker-Content-Digest", &digest)
        .body("Docker blob content".into())
        .unwrap())
}

async fn handle_docker_head_blob(
    State(state): State<AppState>,
    Path((name, digest)): Path<(String, String)>,
) -> Result<Response, StatusCode> {
    // Implementation would use the Docker API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Docker-Content-Digest", &digest)
        .body("".into())
        .unwrap())
}

async fn handle_docker_start_upload(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Response, StatusCode> {
    // Implementation would use the Docker API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::ACCEPTED)
        .header("Location", format!("/v2/{}/blobs/uploads/123e4567-e89b-12d3-a456-426614174000", name))
        .header("Docker-Upload-UUID", "123e4567-e89b-12d3-a456-426614174000")
        .body("".into())
        .unwrap())
}

async fn handle_docker_complete_upload(
    State(state): State<AppState>,
    Path((name, uuid)): Path<(String, String)>,
) -> Result<Response, StatusCode> {
    // Implementation would use the Docker API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .header("Docker-Content-Digest", "sha256:1234567890abcdef")
        .body("".into())
        .unwrap())
}

async fn handle_docker_tags_list(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Response, StatusCode> {
    // Implementation would use the Docker API from distribution crate
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(r#"{"name":"library/nginx","tags":["latest","1.21","1.21.6"]}"#.into())
        .unwrap())
}

// Import the missing handler
use axum::routing::post;