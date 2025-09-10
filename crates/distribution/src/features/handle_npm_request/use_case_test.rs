// crates/distribution/src/features/handle_npm_request/use_case_test.rs

use std::sync::Arc;
use bytes::Bytes;

use crate::domain::npm::{NpmPackageName, NpmVersion, NpmPackageMetadata, NpmPackageJson};
use crate::domain::hrn::{Hrn, RepositoryId};

use super::{
    dto::*,
    ports::*,
    use_case::*,
};

// Tests básicos sin mocks para verificar la estructura
#[tokio::test]
async fn test_get_package_request_creation() {
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let package_name = NpmPackageName::new("test-package").unwrap();
    let version = NpmVersion::new("1.0.0").unwrap();
    let user_id = "test-user".to_string();
    
    let request = GetPackageRequest {
        repository_id: repository_id.clone(),
        package_name: package_name.clone(),
        version: version.clone(),
        user_id: user_id.clone(),
    };
    
    assert_eq!(request.repository_id, repository_id);
    assert_eq!(request.package_name, package_name);
    assert_eq!(request.version, version);
    assert_eq!(request.user_id, user_id);
}

#[tokio::test]
async fn test_put_package_request_creation() {
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let package_name = NpmPackageName::new("test-package").unwrap();
    let version = NpmVersion::new("1.0.0").unwrap();
    let user_id = "test-user".to_string();
    let package_data = Bytes::from("package tarball data");
    
    let metadata = NpmPackageMetadata {
        name: package_name.clone(),
        version: version.clone(),
        description: Some("Test package".to_string()),
        keywords: vec!["test".to_string()],
        author: Some("Test Author".to_string()),
        license: Some("MIT".to_string()),
        dependencies: Default::default(),
        dev_dependencies: Default::default(),
        peer_dependencies: Default::default(),
        dist: NpmDistInfo {
            tarball: format!("{}/test-package/-/test-package-1.0.0.tgz", repository_id.value()),
            shasum: "abc123".to_string(),
            integrity: "sha512-abc123".to_string(),
            file_count: 10,
            unpacked_size: 1024,
        },
    };
    
    let request = PutPackageRequest {
        repository_id: repository_id.clone(),
        package_name: package_name.clone(),
        version: version.clone(),
        package_data: package_data.clone(),
        metadata: metadata.clone(),
        user_id: user_id.clone(),
    };
    
    assert_eq!(request.repository_id, repository_id);
    assert_eq!(request.package_name, package_name);
    assert_eq!(request.version, version);
    assert_eq!(request.package_data, package_data);
    assert_eq!(request.metadata.name, package_name);
    assert_eq!(request.user_id, user_id);
}

#[tokio::test]
async fn test_get_package_json_request_creation() {
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let package_name = NpmPackageName::new("test-package").unwrap();
    let user_id = "test-user".to_string();
    
    let request = GetPackageJsonRequest {
        repository_id: repository_id.clone(),
        package_name: package_name.clone(),
        user_id: user_id.clone(),
    };
    
    assert_eq!(request.repository_id, repository_id);
    assert_eq!(request.package_name, package_name);
    assert_eq!(request.user_id, user_id);
}

#[tokio::test]
async fn test_update_dist_tags_request_creation() {
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let package_name = NpmPackageName::new("test-package").unwrap();
    let user_id = "test-user".to_string();
    
    let mut dist_tags = NpmDistTags::new();
    dist_tags.insert("latest".to_string(), NpmVersion::new("2.0.0").unwrap());
    dist_tags.insert("beta".to_string(), NpmVersion::new("2.1.0-beta.1").unwrap());
    
    let request = UpdateDistTagsRequest {
        repository_id: repository_id.clone(),
        package_name: package_name.clone(),
        dist_tags: dist_tags.clone(),
        user_id: user_id.clone(),
    };
    
    assert_eq!(request.repository_id, repository_id);
    assert_eq!(request.package_name, package_name);
    assert_eq!(request.dist_tags, dist_tags);
    assert_eq!(request.user_id, user_id);
}

#[tokio::test]
async fn test_search_request_creation() {
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let user_id = "test-user".to_string();
    
    let request = SearchRequest {
        repository_id: repository_id.clone(),
        query: "test".to_string(),
        limit: 10,
        user_id: user_id.clone(),
    };
    
    assert_eq!(request.repository_id, repository_id);
    assert_eq!(request.query, "test");
    assert_eq!(request.limit, 10);
    assert_eq!(request.user_id, user_id);
}

#[tokio::test]
async fn test_head_package_request_creation() {
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let package_name = NpmPackageName::new("test-package").unwrap();
    let version = NpmVersion::new("1.0.0").unwrap();
    let user_id = "test-user".to_string();
    
    let request = HeadPackageRequest {
        repository_id: repository_id.clone(),
        package_name: package_name.clone(),
        version: version.clone(),
        user_id: user_id.clone(),
    };
    
    assert_eq!(request.repository_id, repository_id);
    assert_eq!(request.package_name, package_name);
    assert_eq!(request.version, version);
    assert_eq!(request.user_id, user_id);
}

#[tokio::test]
async fn test_get_dist_tags_request_creation() {
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let package_name = NpmPackageName::new("test-package").unwrap();
    let user_id = "test-user".to_string();
    
    let request = GetDistTagsRequest {
        repository_id: repository_id.clone(),
        package_name: package_name.clone(),
        user_id: user_id.clone(),
    };
    
    assert_eq!(request.repository_id, repository_id);
    assert_eq!(request.package_name, package_name);
    assert_eq!(request.user_id, user_id);
}

// Test de respuestas
#[tokio::test]
async fn test_get_package_response_creation() {
    let package_name = NpmPackageName::new("test-package").unwrap();
    let version = NpmVersion::new("1.0.0").unwrap();
    let package_data = Bytes::from("package tarball data");
    
    let response = GetPackageResponse {
        package_name: package_name.clone(),
        version: version.clone(),
        package_data: package_data.clone(),
    };
    
    assert_eq!(response.package_name, package_name);
    assert_eq!(response.version, version);
    assert_eq!(response.package_data, package_data);
}

#[tokio::test]
async fn test_put_package_response_creation() {
    let package_name = NpmPackageName::new("test-package").unwrap();
    let version = NpmVersion::new("1.0.0").unwrap();
    let size_bytes = 1024;
    
    let response = PutPackageResponse {
        package_name: package_name.clone(),
        version: version.clone(),
        size_bytes,
    };
    
    assert_eq!(response.package_name, package_name);
    assert_eq!(response.version, version);
    assert_eq!(response.size_bytes, size_bytes);
}

#[tokio::test]
async fn test_get_package_json_response_creation() {
    let package_json = NpmPackageJson {
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Test package".to_string()),
        main: Some("index.js".to_string()),
        scripts: Default::default(),
        dependencies: Default::default(),
        dev_dependencies: Default::default(),
        peer_dependencies: Default::default(),
        keywords: vec!["test".to_string()],
        author: Some("Test Author".to_string()),
        license: Some("MIT".to_string()),
        repository: None,
        bugs: None,
        homepage: None,
        dist_tags: Default::default(),
        versions: Default::default(),
        time: Default::default(),
    };
    
    let response = GetPackageJsonResponse {
        package_json: package_json.clone(),
    };
    
    assert_eq!(response.package_json.name, package_json.name);
    assert_eq!(response.package_json.version, package_json.version);
}

#[tokio::test]
async fn test_update_dist_tags_response_creation() {
    let package_name = NpmPackageName::new("test-package").unwrap();
    
    let mut dist_tags = NpmDistTags::new();
    dist_tags.insert("latest".to_string(), NpmVersion::new("2.0.0").unwrap());
    dist_tags.insert("beta".to_string(), NpmVersion::new("2.1.0-beta.1").unwrap());
    
    let response = UpdateDistTagsResponse {
        package_name: package_name.clone(),
        dist_tags: dist_tags.clone(),
    };
    
    assert_eq!(response.package_name, package_name);
    assert_eq!(response.dist_tags, dist_tags);
}

#[tokio::test]
async fn test_search_response_creation() {
    let search_results = vec![
        NpmSearchResult {
            package: NpmPackageName::new("test-package").unwrap(),
            version: NpmVersion::new("1.0.0").unwrap(),
            description: Some("Test package".to_string()),
            keywords: vec!["test".to_string()],
            author: Some("Test Author".to_string()),
            date: chrono::Utc::now(),
        },
    ];
    
    let response = SearchResponse {
        results: search_results.clone(),
    };
    
    assert_eq!(response.results.len(), search_results.len());
    assert_eq!(response.results[0].package, search_results[0].package);
}

#[tokio::test]
async fn test_head_package_response_creation() {
    let package_name = NpmPackageName::new("test-package").unwrap();
    let version = NpmVersion::new("1.0.0").unwrap();
    
    let response = HeadPackageResponse {
        package_name: package_name.clone(),
        version: version.clone(),
        exists: true,
    };
    
    assert_eq!(response.package_name, package_name);
    assert_eq!(response.version, version);
    assert!(response.exists);
}

#[tokio::test]
async fn test_get_dist_tags_response_creation() {
    let mut dist_tags = NpmDistTags::new();
    dist_tags.insert("latest".to_string(), NpmVersion::new("2.0.0").unwrap());
    dist_tags.insert("beta".to_string(), NpmVersion::new("2.1.0-beta.1").unwrap());
    
    let response = GetDistTagsResponse {
        dist_tags: dist_tags.clone(),
    };
    
    assert_eq!(response.dist_tags, dist_tags);
}