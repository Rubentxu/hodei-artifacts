//! Simple integration example for the distribution module
//! 
//! This example demonstrates the basic structure of how to integrate
//! the distribution module with a main server, without depending on
//! all the complex dependencies of the full project.

use std::sync::Arc;
use axum::{
    routing::{get, put, head, post},
    Router, Server,
    extract::{Path, State},
    http::{StatusCode, HeaderMap},
    response::Response,
};
use serde_json::json;

/// Simple state for the distribution API
#[derive(Clone)]
struct AppState {
    // In a real implementation, this would contain the distribution service
    distribution_enabled: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create application state
    let app_state = AppState {
        distribution_enabled: true,
    };
    
    // Build the router with all format-specific routes
    let app = create_distribution_router(app_state);
    
    // Start the server
    let addr = "0.0.0.0:8080".parse()?;
    println!("🚀 Distribution server running on {}", addr);
    println!("📦 Supported formats: Maven, npm, Docker");
    println!("🔧 Maven endpoints: /maven/*");
    println!("📋 npm endpoints: /npm/*");
    println!("🐳 Docker endpoints: /v2/*");
    
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
        // Health check
        .route("/health", get(health_check))
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

// Health check handler
async fn health_check(State(state): State<AppState>) -> Result<Response, StatusCode> {
    let status = json!({
        "status": if state.distribution_enabled { "healthy" } else { "disabled" },
        "formats": ["maven", "npm", "docker"],
        "version": "1.0.0"
    });
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&status).unwrap().into())
        .unwrap())
}

// Maven handlers
async fn handle_maven_get(
    State(state): State<AppState>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    println!("📦 Maven GET request for: {}", path);
    
    // Example response for a Maven artifact download
    let artifact_content = format!("<!-- Maven artifact: {} -->\n<project>...</project>", path);
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/xml")
        .header("Content-Length", artifact_content.len().to_string())
        .body(artifact_content.into())
        .unwrap())
}

async fn handle_maven_put(
    State(state): State<AppState>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    println!("📦 Maven PUT request for: {}", path);
    
    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .header("Location", format!("/maven/{}", path))
        .body("Maven artifact uploaded successfully".into())
        .unwrap())
}

async fn handle_maven_head(
    State(state): State<AppState>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    println!("📦 Maven HEAD request for: {}", path);
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/xml")
        .header("Content-Length", "1024")
        .body("".into())
        .unwrap())
}

async fn handle_maven_metadata(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<Response, StatusCode> {
    println!("📋 Maven metadata request for: {}", path);
    
    let metadata = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>com.example</groupId>
  <artifactId>example-artifact</artifactId>
  <versioning>
    <latest>1.0.0</latest>
    <release>1.0.0</release>
    <versions>
      <version>1.0.0</version>
    </versions>
  </versioning>
</metadata>"#);
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/xml")
        .body(metadata.into())
        .unwrap())
}

// npm handlers
async fn handle_npm_get_package(
    State(state): State<AppState>,
    Path(package): Path<String>,
) -> Result<Response, StatusCode> {
    println!("📋 npm GET package request for: {}", package);
    
    let package_json = json!({
        "name": package,
        "version": "1.0.0",
        "description": "Example npm package",
        "main": "index.js",
        "scripts": {
            "test": "echo \"Error: no test specified\" && exit 1"
        },
        "keywords": ["example"],
        "author": "Example Author",
        "license": "MIT"
    });
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&package_json).unwrap().into())
        .unwrap())
}

async fn handle_npm_put_package(
    State(state): State<AppState>,
    Path(package): Path<String>,
) -> Result<Response, StatusCode> {
    println!("📋 npm PUT package request for: {}", package);
    
    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .header("Location", format!("/npm/{}", package))
        .body("npm package published successfully".into())
        .unwrap())
}

async fn handle_npm_package_json(
    State(state): State<AppState>,
    Path(package): Path<String>,
) -> Result<Response, StatusCode> {
    println!("📋 npm package.json request for: {}", package);
    
    let package_json = json!({
        "name": package,
        "dist-tags": {
            "latest": "1.0.0"
        },
        "versions": {
            "1.0.0": {
                "name": package,
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("http://localhost:8080/npm/{}/-/{}.tgz", package, package),
                    "integrity": "sha512-..."
                }
            }
        }
    });
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&package_json).unwrap().into())
        .unwrap())
}

async fn handle_npm_get_scoped(
    State(state): State<AppState>,
    Path((scope, package)): Path<(String, String)>,
) -> Result<Response, StatusCode> {
    println!("📋 npm GET scoped package request for: @{}/{}", scope, package);
    
    let package_json = json!({
        "name": format!("@{}/{}", scope, package),
        "version": "1.0.0",
        "description": "Example scoped npm package",
        "main": "index.js",
        "license": "MIT"
    });
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&package_json).unwrap().into())
        .unwrap())
}

async fn handle_npm_put_scoped(
    State(state): State<AppState>,
    Path((scope, package)): Path<(String, String)>,
) -> Result<Response, StatusCode> {
    println!("📋 npm PUT scoped package request for: @{}/{}", scope, package);
    
    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .header("Location", format!("/npm/{}/{}", scope, package))
        .body("Scoped npm package published successfully".into())
        .unwrap())
}

// Docker handlers
async fn handle_docker_version() -> Result<Response, StatusCode> {
    println!("🐳 Docker version check");
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Docker-Distribution-Api-Version", "registry/2.0")
        .body("".into())
        .unwrap())
}

async fn handle_docker_catalog(
    State(state): State<AppState>,
) -> Result<Response, StatusCode> {
    println!("🐳 Docker catalog request");
    
    let catalog = json!({
        "repositories": [
            "library/nginx",
            "library/redis",
            "library/postgres"
        ]
    });
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&catalog).unwrap().into())
        .unwrap())
}

async fn handle_docker_get_manifest(
    State(state): State<AppState>,
    Path((name, reference)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    println!("🐳 Docker GET manifest request for: {}:{}", name, reference);
    
    let manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
        "config": {
            "mediaType": "application/vnd.docker.container.image.v1+json",
            "size": 7023,
            "digest": "sha256:b5b2b2c507a0944348e0303114d8d93aaaa081732b86451d9bce1f432a537bc7"
        },
        "layers": [
            {
                "mediaType": "application/vnd.docker.image.rootfs.diff.tar.gzip",
                "size": 32654,
                "digest": "sha256:e692418e4cbaf90ca69d05a66403747baa33ee08806650b51fab815ad7fc331f"
            }
        ]
    });
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/vnd.docker.distribution.manifest.v2+json")
        .header("Docker-Content-Digest", "sha256:1234567890abcdef")
        .body(serde_json::to_string(&manifest).unwrap().into())
        .unwrap())
}

async fn handle_docker_put_manifest(
    State(state): State<AppState>,
    Path((name, reference)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    println!("🐳 Docker PUT manifest request for: {}:{}", name, reference);
    
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
    println!("🐳 Docker HEAD manifest request for: {}:{}", name, reference);
    
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
    println!("🐳 Docker GET blob request for: {}@{}", name, digest);
    
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
    println!("🐳 Docker HEAD blob request for: {}@{}", name, digest);
    
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
    println!("🐳 Docker start upload request for: {}", name);
    
    let upload_uuid = "123e4567-e89b-12d3-a456-426614174000";
    
    Ok(Response::builder()
        .status(StatusCode::ACCEPTED)
        .header("Location", format!("/v2/{}/blobs/uploads/{}", name, upload_uuid))
        .header("Docker-Upload-UUID", upload_uuid)
        .range_supported(true)
        .body("".into())
        .unwrap())
}

async fn handle_docker_complete_upload(
    State(state): State<AppState>,
    Path((name, uuid)): Path<(String, String)>,
) -> Result<Response, StatusCode> {
    println!("🐳 Docker complete upload request for: {} with UUID: {}", name, uuid);
    
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
    println!("🐳 Docker tags list request for: {}", name);
    
    let tags = json!({
        "name": name,
        "tags": [
            "latest",
            "1.21",
            "1.21.6",
            "1.20",
            "1.19"
        ]
    });
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&tags).unwrap().into())
        .unwrap())
}