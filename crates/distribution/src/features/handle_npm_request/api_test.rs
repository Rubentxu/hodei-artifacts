// crates/distribution/src/features/handle_npm_request/api_test.rs

use std::sync::Arc;
use bytes::Bytes;

use crate::domain::npm::{NpmPackageName, NpmVersion, NpmPackageMetadata, NpmPackageJson, NpmDistInfo, NpmDistTags};
use crate::domain::hrn::{Hrn, RepositoryId};

use super::{
    dto::*,
    api::*,
};

// Mock simple para tests básicos
struct MockNpmPackageEndpoint;

impl MockNpmPackageEndpoint {
    fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl NpmPackageApi for MockNpmPackageEndpoint {
    async fn get_package(&self, request: GetPackageRequest) -> Result<GetPackageResponse, NpmPackageApiError> {
        // Mock response para testing
        Ok(GetPackageResponse {
            package_data: Bytes::from("mock package data"),
            metadata: NpmPackageMetadata {
                name: request.package_name,
                version: request.version,
                description: Some("Mock package".to_string()),
                keywords: vec!["test".to_string()],
                author: Some("Test Author".to_string()),
                license: Some("MIT".to_string()),
                dependencies: Default::default(),
                dev_dependencies: Default::default(),
                peer_dependencies: Default::default(),
                dist: NpmDistInfo {
                    tarball: format!("{}/test-package/-/test-package-1.0.0.tgz", request.repository_id.value()),
                    shasum: "abc123".to_string(),
                    integrity: "sha512-abc123".to_string(),
                    file_count: 10,
                    unpacked_size: 1024,
                },
            },
            content_type: "application/octet-stream".to_string(),
            etag: Some("etag123".to_string()),
            last_modified: Some("2024-01-01T00:00:00Z".to_string()),
        })
    }

    async fn put_package(&self, request: PutPackageRequest) -> Result<PutPackageResponse, NpmPackageApiError> {
        Ok(PutPackageResponse {
            success: true,
            package_url: format!("{}/{}/-/{}-{}.tgz",
                request.repository_id.value(),
                request.package_name.value(),
                request.package_name.value(),
                request.version.value()
            ),
            published_at: "2024-01-01T00:00:00Z".to_string(),
        })
    }

    async fn head_package(&self, request: HeadPackageRequest) -> Result<HeadPackageResponse, NpmPackageApiError> {
        Ok(HeadPackageResponse {
            exists: true,
            content_length: 1024,
            content_type: "application/octet-stream".to_string(),
            etag: Some("etag123".to_string()),
            last_modified: Some("2024-01-01T00:00:00Z".to_string()),
        })
    }

    async fn get_package_json(&self, request: GetPackageJsonRequest) -> Result<GetPackageJsonResponse, NpmPackageApiError> {
        let mut versions = std::collections::HashMap::new();
        versions.insert("1.0.0".to_string(), NpmPackageJson {
            name: request.package_name.clone(),
            version: NpmVersion::new("1.0.0").unwrap(),
            description: Some("Test package".to_string()),
            main: Some("index.js".to_string()),
            scripts: Default::default(),
            dependencies: Default::default(),
            dev_dependencies: Default::default(),
            peer_dependencies: Default::default(),
            keywords: vec!["test".to_string()],
            author: Some("Test Author".to_string()),
            license: Some("MIT".to_string()),
            repository: Some("https://github.com/test/test-package".to_string()),
            bugs: Some("https://github.com/test/test-package/issues".to_string()),
            homepage: Some("https://github.com/test/test-package#readme".to_string()),
        });

        let mut dist_tags = NpmDistTags::new();
        dist_tags.insert("latest".to_string(), NpmVersion::new("1.0.0").unwrap());

        Ok(GetPackageJsonResponse {
            name: request.package_name,
            versions,
            dist_tags,
            time: std::collections::HashMap::new(),
        })
    }

    async fn update_dist_tags(&self, request: UpdateDistTagsRequest) -> Result<UpdateDistTagsResponse, NpmPackageApiError> {
        Ok(UpdateDistTagsResponse {
            success: true,
            updated_tags: request.dist_tags,
        })
    }

    async fn get_dist_tags(&self, request: GetDistTagsRequest) -> Result<GetDistTagsResponse, NpmPackageApiError> {
        let mut dist_tags = NpmDistTags::new();
        dist_tags.insert("latest".to_string(), NpmVersion::new("1.0.0").unwrap());
        
        Ok(GetDistTagsResponse {
            dist_tags,
        })
    }

    async fn search_packages(&self, request: SearchRequest) -> Result<SearchResponse, NpmPackageApiError> {
        Ok(SearchResponse {
            packages: vec![],
            total: 0,
        })
    }
}

// Tests básicos para verificar la estructura del API
#[tokio::test]
async fn test_npm_package_api_get_success() {
    let endpoint = MockNpmPackageEndpoint::new();
    
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
    
    let result = endpoint.get_package(request).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert_eq!(response.metadata.name, package_name);
    assert_eq!(response.metadata.version, version);
    assert_eq!(response.content_type, "application/octet-stream");
}

#[tokio::test]
async fn test_npm_package_api_put_success() {
    let endpoint = MockNpmPackageEndpoint::new();
    
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
    
    let result = endpoint.put_package(request).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert!(response.success);
    assert!(response.package_url.contains(&package_name.value()));
    assert!(response.package_url.contains(&version.value()));
}

#[tokio::test]
async fn test_npm_package_api_head_success() {
    let endpoint = MockNpmPackageEndpoint::new();
    
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
    
    let result = endpoint.head_package(request).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert!(response.exists);
    assert_eq!(response.content_length, 1024);
    assert_eq!(response.content_type, "application/octet-stream");
}

#[tokio::test]
async fn test_npm_package_api_get_package_json_success() {
    let endpoint = MockNpmPackageEndpoint::new();
    
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let package_name = NpmPackageName::new("test-package").unwrap();
    let user_id = "test-user".to_string();
    
    let request = GetPackageJsonRequest {
        repository_id: repository_id.clone(),
        package_name: package_name.clone(),
        user_id: user_id.clone(),
    };
    
    let result = endpoint.get_package_json(request).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert_eq!(response.name, package_name);
    assert!(response.versions.contains_key("1.0.0"));
    assert!(response.dist_tags.contains_key("latest"));
}

#[tokio::test]
async fn test_npm_package_api_update_dist_tags_success() {
    let endpoint = MockNpmPackageEndpoint::new();
    
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
    
    let result = endpoint.update_dist_tags(request).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert!(response.success);
    assert_eq!(response.updated_tags.len(), 2);
}

#[tokio::test]
async fn test_npm_package_api_get_dist_tags_success() {
    let endpoint = MockNpmPackageEndpoint::new();
    
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let package_name = NpmPackageName::new("test-package").unwrap();
    let user_id = "test-user".to_string();
    
    let request = GetDistTagsRequest {
        repository_id: repository_id.clone(),
        package_name: package_name.clone(),
        user_id: user_id.clone(),
    };
    
    let result = endpoint.get_dist_tags(request).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert!(response.dist_tags.contains_key("latest"));
}

#[tokio::test]
async fn test_npm_package_api_search_success() {
    let endpoint = MockNpmPackageEndpoint::new();
    
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let user_id = "test-user".to_string();
    
    let request = SearchRequest {
        repository_id: repository_id.clone(),
        query: "test".to_string(),
        limit: 10,
        user_id: user_id.clone(),
    };
    
    let result = endpoint.search_packages(request).await;
    assert!(result.is_ok());
    
    let response = result.unwrap();
    assert_eq!(response.packages.len(), 0);
    assert_eq!(response.total, 0);
}

#[tokio::test]
async fn test_npm_package_api_put_creation() {
    let endpoint = NpmPackageEndpoint::new();
    
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
    
    let result = endpoint.put_package(request).await;
    assert!(result.is_err()); // Esperamos error porque no hay implementación real
}

#[tokio::test]
async fn test_npm_package_api_get_package_json() {
    let endpoint = NpmPackageEndpoint::new();
    
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let package_name = NpmPackageName::new("test-package").unwrap();
    let user_id = "test-user".to_string();
    
    let request = GetPackageJsonRequest {
        repository_id: repository_id.clone(),
        package_name: package_name.clone(),
        user_id: user_id.clone(),
    };
    
    let result = endpoint.get_package_json(request).await;
    assert!(result.is_err()); // Esperamos error porque no hay implementación real
}

#[tokio::test]
async fn test_npm_package_api_update_dist_tags() {
    let endpoint = NpmPackageEndpoint::new();
    
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
    
    let result = endpoint.update_dist_tags(request).await;
    assert!(result.is_err()); // Esperamos error porque no hay implementación real
}

#[tokio::test]
async fn test_npm_package_api_search() {
    let endpoint = NpmPackageEndpoint::new();
    
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let user_id = "test-user".to_string();
    
    let request = SearchRequest {
        repository_id: repository_id.clone(),
        query: "test".to_string(),
        limit: 10,
        user_id: user_id.clone(),
    };
    
    let result = endpoint.search_packages(request).await;
    assert!(result.is_err()); // Esperamos error porque no hay implementación real
}

#[tokio::test]
async fn test_npm_package_api_head_package() {
    let endpoint = NpmPackageEndpoint::new();
    
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
    
    let result = endpoint.head_package(request).await;
    assert!(result.is_err()); // Esperamos error porque no hay implementación real
}

#[tokio::test]
async fn test_npm_package_api_get_dist_tags() {
    let endpoint = NpmPackageEndpoint::new();
    
    let repository_id = RepositoryId::new("npm-repo").unwrap();
    let package_name = NpmPackageName::new("test-package").unwrap();
    let user_id = "test-user".to_string();
    
    let request = GetDistTagsRequest {
        repository_id: repository_id.clone(),
        package_name: package_name.clone(),
        user_id: user_id.clone(),
    };
    
    let result = endpoint.get_dist_tags(request).await;
    assert!(result.is_err()); // Esperamos error porque no hay implementación real
}