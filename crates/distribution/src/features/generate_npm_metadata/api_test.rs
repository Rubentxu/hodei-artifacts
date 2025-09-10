use super::*;
use crate::features::generate_npm_metadata::ports::{
    MockNpmMetadataGenerator, MockNpmPackageLister, MockNpmMetadataCache,
};
use std::sync::Arc;

#[tokio::test]
async fn test_generate_npm_metadata_api_success() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let lister = Arc::new(MockNpmPackageLister::new());
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let api = GenerateNpmMetadataApi::new(generator.clone(), lister.clone(), cache.clone());
    
    let request = GenerateNpmMetadataRequest {
        scope: None,
        package_name: "test-package".to_string(),
        repository_id: "repo-123".to_string(),
        force_regenerate: false,
    };
    
    let result = api.generate_npm_metadata(request).await;
    
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.metadata.name, "test-package");
    assert_eq!(response.metadata.version, "1.0.0");
    assert!(!response.cache_hit);
}

#[tokio::test]
async fn test_generate_npm_metadata_api_validation_error() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let lister = Arc::new(MockNpmPackageLister::new());
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let api = GenerateNpmMetadataApi::new(generator, lister, cache);
    
    // Test with invalid package name
    let request = GenerateNpmMetadataRequest {
        scope: None,
        package_name: "".to_string(), // Invalid empty package name
        repository_id: "repo-123".to_string(),
        force_regenerate: false,
    };
    
    let result = api.generate_npm_metadata(request).await;
    
    assert!(result.is_err());
    match result.unwrap_err() {
        GenerateNpmMetadataError::InvalidPackageName { name } => {
            assert_eq!(name, "");
        }
        _ => panic!("Expected InvalidPackageName error"),
    }
}

#[tokio::test]
async fn test_generate_npm_metadata_api_generator_error() {
    let mut generator = MockNpmMetadataGenerator::new();
    generator.set_should_fail(true);
    
    let generator = Arc::new(generator);
    let lister = Arc::new(MockNpmPackageLister::new());
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let api = GenerateNpmMetadataApi::new(generator, lister, cache);
    
    let request = GenerateNpmMetadataRequest {
        scope: None,
        package_name: "test-package".to_string(),
        repository_id: "repo-123".to_string(),
        force_regenerate: false,
    };
    
    let result = api.generate_npm_metadata(request).await;
    
    assert!(result.is_err());
    match result.unwrap_err() {
        GenerateNpmMetadataError::MetadataGenerationFailed { reason } => {
            assert!(reason.contains("Mock generator error"));
        }
        _ => panic!("Expected MetadataGenerationFailed"),
    }
}

#[tokio::test]
async fn test_generate_npm_metadata_api_lister_error() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let mut lister = MockNpmPackageLister::new();
    lister.set_should_fail(true);
    let lister = Arc::new(lister);
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let api = GenerateNpmMetadataApi::new(generator, lister, cache);
    
    let request = GenerateNpmMetadataRequest {
        scope: None,
        package_name: "test-package".to_string(),
        repository_id: "repo-123".to_string(),
        force_regenerate: false,
    };
    
    let result = api.generate_npm_metadata(request).await;
    
    assert!(result.is_err());
    match result.unwrap_err() {
        GenerateNpmMetadataError::RepositoryError(msg) => {
            assert!(msg.contains("Mock lister error"));
        }
        _ => panic!("Expected RepositoryError"),
    }
}

#[tokio::test]
async fn test_generate_npm_metadata_api_cache_error() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let lister = Arc::new(MockNpmPackageLister::new());
    let mut cache = MockNpmMetadataCache::new();
    cache.set_should_fail(true);
    let cache = Arc::new(cache);
    
    let api = GenerateNpmMetadataApi::new(generator, lister, cache);
    
    let request = GenerateNpmMetadataRequest {
        scope: None,
        package_name: "test-package".to_string(),
        repository_id: "repo-123".to_string(),
        force_regenerate: false,
    };
    
    let result = api.generate_npm_metadata(request).await;
    
    assert!(result.is_err());
    match result.unwrap_err() {
        GenerateNpmMetadataError::CacheError(msg) => {
            assert!(msg.contains("Mock cache error"));
        }
        _ => panic!("Expected CacheError"),
    }
}

#[tokio::test]
async fn test_generate_npm_metadata_api_scoped_package() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let lister = Arc::new(MockNpmPackageLister::new());
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let api = GenerateNpmMetadataApi::new(generator.clone(), lister.clone(), cache.clone());
    
    let request = GenerateNpmMetadataRequest {
        scope: Some("@scope".to_string()),
        package_name: "test-package".to_string(),
        repository_id: "repo-123".to_string(),
        force_regenerate: false,
    };
    
    let result = api.generate_npm_metadata(request).await;
    
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.metadata.name, "@scope/test-package");
    assert!(!response.cache_hit);
}

#[tokio::test]
async fn test_generate_npm_metadata_api_with_dist_tags() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let lister = Arc::new(MockNpmPackageLister::new());
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let api = GenerateNpmMetadataApi::new(generator.clone(), lister.clone(), cache.clone());
    
    let request = GenerateNpmMetadataRequest {
        scope: None,
        package_name: "test-package".to_string(),
        repository_id: "repo-123".to_string(),
        force_regenerate: false,
    };
    
    let result = api.generate_npm_metadata(request).await;
    
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.metadata.name, "test-package");
    assert!(response.metadata.dist_tags.contains_key("latest"));
    assert!(response.metadata.versions.contains(&"1.0.0".to_string()));
    assert!(!response.cache_hit);
}

#[tokio::test]
async fn test_generate_npm_metadata_api_caching_behavior() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let lister = Arc::new(MockNpmPackageLister::new());
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let api = GenerateNpmMetadataApi::new(generator.clone(), lister.clone(), cache.clone());
    
    let request = GenerateNpmMetadataRequest {
        scope: None,
        package_name: "test-package".to_string(),
        repository_id: "repo-123".to_string(),
        force_regenerate: false,
    };
    
    // First call - should generate and cache
    let result1 = api.generate_npm_metadata(request.clone()).await;
    assert!(result1.is_ok());
    
    // Second call - should use cache
    let result2 = api.generate_npm_metadata(request).await;
    assert!(result2.is_ok());
    
    // Verify cache was used
    assert!(cache.get_call_count() > 0);
}