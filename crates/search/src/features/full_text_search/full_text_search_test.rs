use std::sync::Arc;
use tokio;

use crate::features::full_text_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    use_case::FullTextSearchUseCase,
    test_utils::{MockSearchIndexAdapter, MockArtifactRepositoryAdapter, MockEventPublisherAdapter},
    error::FullTextSearchError,
};

#[tokio::test]
async fn test_full_text_search_with_results() {
    // Arrange
    let search_index = Arc::new(MockSearchIndexAdapter::new());
    let artifact_repository = Arc::new(MockArtifactRepositoryAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    // Add some test data
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
        description: "A test package".to_string(),
        content: "This is test content".to_string(),
        score: 1.0,
    }).await;
    
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-2".to_string(),
        name: "another-package".to_string(),
        version: "2.0.0".to_string(),
        package_type: "maven".to_string(),
        repository: "test-repo".to_string(),
        description: "Another test package".to_string(),
        content: "This is another test content".to_string(),
        score: 0.8,
    }).await;
    
    let use_case = FullTextSearchUseCase::new(
        search_index.clone(),
        artifact_repository,
        event_publisher,
    );
    
    // Act
    let query = SearchQuery {
        q: "test".to_string(),
        page: Some(1),
        page_size: Some(10),
        language: None,
        fields: None,
    };
    
    let results = use_case.execute(query).await.unwrap();
    
    // Assert
    assert_eq!(results.total_count, 2);
    assert_eq!(results.artifacts.len(), 2);
    assert_eq!(results.artifacts[0].name, "test-package");
    assert_eq!(results.artifacts[1].name, "another-package");
}

#[tokio::test]
async fn test_empty_search_returns_all_artifacts() {
    // Arrange
    let search_index = Arc::new(MockSearchIndexAdapter::new());
    let artifact_repository = Arc::new(MockArtifactRepositoryAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    // Add some test data
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
        description: "A test package".to_string(),
        content: "This is test content".to_string(),
        score: 1.0,
    }).await;
    
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-2".to_string(),
        name: "another-package".to_string(),
        version: "2.0.0".to_string(),
        package_type: "maven".to_string(),
        repository: "test-repo".to_string(),
        description: "Another test package".to_string(),
        content: "This is another test content".to_string(),
        score: 0.8,
    }).await;
    
    let use_case = FullTextSearchUseCase::new(
        search_index.clone(),
        artifact_repository,
        event_publisher,
    );
    
    // Act
    let query = SearchQuery {
        q: "".to_string(),
        page: Some(1),
        page_size: Some(10),
        language: None,
        fields: None,
    };
    
    let results = use_case.execute(query).await.unwrap();
    
    // Assert
    assert_eq!(results.total_count, 2);
    assert_eq!(results.artifacts.len(), 2);
}

#[tokio::test]
async fn test_case_insensitive_search() {
    // Arrange
    let search_index = Arc::new(MockSearchIndexAdapter::new());
    let artifact_repository = Arc::new(MockArtifactRepositoryAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    // Add some test data with mixed case
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "Test-Package".to_string(), // Mixed case
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
        description: "A test package".to_string(),
        content: "This is test content".to_string(),
        score: 1.0,
    }).await;
    
    let use_case = FullTextSearchUseCase::new(
        search_index.clone(),
        artifact_repository,
        event_publisher,
    );
    
    // Act
    let query = SearchQuery {
        q: "test-package".to_string(), // Lowercase query
        page: Some(1),
        page_size: Some(10),
        language: None,
        fields: None,
    };
    
    let results = use_case.execute(query).await.unwrap();
    
    // Assert
    assert_eq!(results.total_count, 1);
    assert_eq!(results.artifacts.len(), 1);
    assert_eq!(results.artifacts[0].name, "Test-Package");
}

#[tokio::test]
async fn test_search_with_numbers() {
    // Arrange
    let search_index = Arc::new(MockSearchIndexAdapter::new());
    let artifact_repository = Arc::new(MockArtifactRepositoryAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    // Add some test data with numbers
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "package123".to_string(),
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
        description: "A numbered package".to_string(),
        content: "This is numbered content 123".to_string(),
        score: 1.0,
    }).await;
    
    let use_case = FullTextSearchUseCase::new(
        search_index.clone(),
        artifact_repository,
        event_publisher,
    );
    
    // Act
    let query = SearchQuery {
        q: "123".to_string(),
        page: Some(1),
        page_size: Some(10),
        language: None,
        fields: None,
    };
    
    let results = use_case.execute(query).await.unwrap();
    
    // Assert
    assert_eq!(results.total_count, 1);
    assert_eq!(results.artifacts.len(), 1);
    assert_eq!(results.artifacts[0].name, "package123");
}

#[tokio::test]
async fn test_search_version_exact_match() {
    // Arrange
    let search_index = Arc::new(MockSearchIndexAdapter::new());
    let artifact_repository = Arc::new(MockArtifactRepositoryAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    // Add some test data
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "test-package".to_string(),
        version: "2.1.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
        description: "A test package".to_string(),
        content: "This is test content".to_string(),
        score: 1.0,
    }).await;
    
    let use_case = FullTextSearchUseCase::new(
        search_index.clone(),
        artifact_repository,
        event_publisher,
    );
    
    // Act
    let query = SearchQuery {
        q: "2.1.0".to_string(),
        page: Some(1),
        page_size: Some(10),
        language: None,
        fields: None,
    };
    
    let results = use_case.execute(query).await.unwrap();
    
    // Assert
    assert_eq!(results.total_count, 1);
    assert_eq!(results.artifacts.len(), 1);
    assert_eq!(results.artifacts[0].version, "2.1.0");
}

#[tokio::test]
async fn test_search_with_special_characters() {
    // Arrange
    let search_index = Arc::new(MockSearchIndexAdapter::new());
    let artifact_repository = Arc::new(MockArtifactRepositoryAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    // Add some test data with special characters
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "test-package-with-dashes".to_string(),
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
        description: "A test package with dashes".to_string(),
        content: "This is test content with dashes".to_string(),
        score: 1.0,
    }).await;
    
    let use_case = FullTextSearchUseCase::new(
        search_index.clone(),
        artifact_repository,
        event_publisher,
    );
    
    // Act
    let query = SearchQuery {
        q: "test-package-with-dashes".to_string(),
        page: Some(1),
        page_size: Some(10),
        language: None,
        fields: None,
    };
    
    let results = use_case.execute(query).await.unwrap();
    
    // Assert
    assert_eq!(results.total_count, 1);
    assert_eq!(results.artifacts.len(), 1);
    assert_eq!(results.artifacts[0].name, "test-package-with-dashes");
}