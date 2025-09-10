use super::*;
use crate::features::generate_npm_metadata::ports::{
    MockNpmMetadataGenerator, MockNpmPackageLister, MockNpmMetadataCache,
};
use std::sync::Arc;

#[tokio::test]
async fn test_generate_npm_metadata_use_case_success() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let lister = Arc::new(MockNpmPackageLister::new());
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let use_case = GenerateNpmMetadataUseCase::new(generator.clone(), lister.clone(), cache.clone());
    
    let command = GenerateNpmMetadataCommand {
        repository_id: "repo-123".to_string(),
        package_name: "test-package".to_string(),
        registry_url: "https://registry.npmjs.org".to_string(),
    };
    
    let result = use_case.execute(command).await;
    
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.repository_id, "repo-123");
    assert_eq!(response.package_name, "test-package");
    assert!(response.metadata.is_some());
}

#[tokio::test]
async fn test_generate_npm_metadata_use_case_validation_error() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let lister = Arc::new(MockNpmPackageLister::new());
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let use_case = GenerateNpmMetadataUseCase::new(generator, lister, cache);
    
    // Test with invalid package name
    let command = GenerateNpmMetadataCommand {
        repository_id: "repo-123".to_string(),
        package_name: "".to_string(), // Invalid empty package name
        registry_url: "https://registry.npmjs.org".to_string(),
    };
    
    let result = use_case.execute(command).await;
    
    assert!(result.is_err());
    match result.unwrap_err() {
        GenerateNpmMetadataError::ValidationError(msg) => {
            assert!(msg.contains("Package name cannot be empty"));
        }
        _ => panic!("Expected ValidationError"),
    }
}

#[tokio::test]
async fn test_generate_npm_metadata_use_case_generator_error() {
    let mut generator = MockNpmMetadataGenerator::new();
    generator.set_should_fail(true);
    
    let generator = Arc::new(generator);
    let lister = Arc::new(MockNpmPackageLister::new());
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let use_case = GenerateNpmMetadataUseCase::new(generator, lister, cache);
    
    let command = GenerateNpmMetadataCommand {
        repository_id: "repo-123".to_string(),
        package_name: "test-package".to_string(),
        registry_url: "https://registry.npmjs.org".to_string(),
    };
    
    let result = use_case.execute(command).await;
    
    assert!(result.is_err());
    match result.unwrap_err() {
        GenerateNpmMetadataError::GenerationFailed(msg) => {
            assert!(msg.contains("Mock generator error"));
        }
        _ => panic!("Expected GenerationFailed"),
    }
}

#[tokio::test]
async fn test_generate_npm_metadata_use_case_lister_error() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let mut lister = MockNpmPackageLister::new();
    lister.set_should_fail(true);
    let lister = Arc::new(lister);
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let use_case = GenerateNpmMetadataUseCase::new(generator, lister, cache);
    
    let command = GenerateNpmMetadataCommand {
        repository_id: "repo-123".to_string(),
        package_name: "test-package".to_string(),
        registry_url: "https://registry.npmjs.org".to_string(),
    };
    
    let result = use_case.execute(command).await;
    
    assert!(result.is_err());
    match result.unwrap_err() {
        GenerateNpmMetadataError::PackageListingFailed(msg) => {
            assert!(msg.contains("Mock lister error"));
        }
        _ => panic!("Expected PackageListingFailed"),
    }
}

#[tokio::test]
async fn test_generate_npm_metadata_use_case_cache_error() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let lister = Arc::new(MockNpmPackageLister::new());
    let mut cache = MockNpmMetadataCache::new();
    cache.set_should_fail(true);
    let cache = Arc::new(cache);
    
    let use_case = GenerateNpmMetadataUseCase::new(generator, lister, cache);
    
    let command = GenerateNpmMetadataCommand {
        repository_id: "repo-123".to_string(),
        package_name: "test-package".to_string(),
        registry_url: "https://registry.npmjs.org".to_string(),
    };
    
    let result = use_case.execute(command).await;
    
    assert!(result.is_err());
    match result.unwrap_err() {
        GenerateNpmMetadataError::CacheOperationFailed(msg) => {
            assert!(msg.contains("Mock cache error"));
        }
        _ => panic!("Expected CacheOperationFailed"),
    }
}

#[tokio::test]
async fn test_generate_npm_metadata_use_case_scoped_package() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let lister = Arc::new(MockNpmPackageLister::new());
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let use_case = GenerateNpmMetadataUseCase::new(generator.clone(), lister.clone(), cache.clone());
    
    let command = GenerateNpmMetadataCommand {
        repository_id: "repo-123".to_string(),
        package_name: "@scope/test-package".to_string(),
        registry_url: "https://registry.npmjs.org".to_string(),
    };
    
    let result = use_case.execute(command).await;
    
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.package_name, "@scope/test-package");
    assert!(response.metadata.is_some());
}

#[tokio::test]
async fn test_generate_npm_metadata_use_case_with_dist_tags() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let lister = Arc::new(MockNpmPackageLister::new());
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let use_case = GenerateNpmMetadataUseCase::new(generator.clone(), lister.clone(), cache.clone());
    
    let command = GenerateNpmMetadataCommand {
        repository_id: "repo-123".to_string(),
        package_name: "test-package".to_string(),
        registry_url: "https://registry.npmjs.org".to_string(),
    };
    
    let result = use_case.execute(command).await;
    
    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.metadata.is_some());
    
    let metadata = response.metadata.unwrap();
    assert!(metadata.dist_tags.contains_key("latest"));
    assert!(metadata.versions.contains_key("1.0.0"));
}

#[tokio::test]
async fn test_generate_npm_metadata_use_case_caching_behavior() {
    let generator = Arc::new(MockNpmMetadataGenerator::new());
    let lister = Arc::new(MockNpmPackageLister::new());
    let cache = Arc::new(MockNpmMetadataCache::new());
    
    let use_case = GenerateNpmMetadataUseCase::new(generator.clone(), lister.clone(), cache.clone());
    
    let command = GenerateNpmMetadataCommand {
        repository_id: "repo-123".to_string(),
        package_name: "test-package".to_string(),
        registry_url: "https://registry.npmjs.org".to_string(),
    };
    
    // First call - should generate and cache
    let result1 = use_case.execute(command.clone()).await;
    assert!(result1.is_ok());
    
    // Second call - should use cache
    let result2 = use_case.execute(command).await;
    assert!(result2.is_ok());
    
    // Verify cache was used
    assert!(cache.get_call_count() > 0);
}