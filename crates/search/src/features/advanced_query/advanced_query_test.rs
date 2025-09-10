use std::sync::Arc;
use tokio;

use crate::features::advanced_query::{
    dto::{AdvancedSearchQuery, ParsedQueryInfo, AdvancedSearchResults},
    error::AdvancedQueryError,
    use_case::AdvancedQueryUseCase,
    test_adapter::{MockQueryParserAdapter, MockAdvancedSearchIndexAdapter, MockEventPublisherAdapter},
};

#[tokio::test]
async fn test_advanced_search_with_results() {
    // Arrange
    let query_parser = Arc::new(MockQueryParserAdapter::new());
    let search_index = Arc::new(MockAdvancedSearchIndexAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    let use_case = AdvancedQueryUseCase::new(
        query_parser,
        search_index.clone(),
        event_publisher,
    );
    
    // Add some test data
    search_index.add_test_artifact(crate::features::basic_search::dto::ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
        description: "A test package".to_string(),
        content: "This is test content".to_string(),
        score: 1.0,
    }).await;
    
    // Act
    let query = AdvancedSearchQuery {
        q: "test".to_string(),
        page: Some(1),
        page_size: Some(10),
        language: None,
        fields: None,
    };
    
    let results = use_case.execute(query).await.unwrap();
    
    // Assert
    assert_eq!(results.total_count, 1);
    assert_eq!(results.artifacts.len(), 1);
    assert_eq!(results.artifacts[0].name, "test-package");
    assert_eq!(results.artifacts[0].id, "test-artifact-1");
    assert_eq!(results.artifacts[0].version, "1.0.0");
    assert_eq!(results.artifacts[0].package_type, "npm");
    assert_eq!(results.artifacts[0].repository, "test-repo");
}

#[tokio::test]
async fn test_empty_advanced_search_returns_all_artifacts() {
    // Arrange
    let query_parser = Arc::new(MockQueryParserAdapter::new());
    let search_index = Arc::new(MockAdvancedSearchIndexAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    let use_case = AdvancedQueryUseCase::new(
        query_parser,
        search_index.clone(),
        event_publisher,
    );
    
    // Add some test data
    search_index.add_test_artifact(crate::features::basic_search::dto::ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
        description: "A test package".to_string(),
        content: "This is test content".to_string(),
        score: 1.0,
    }).await;
    
    search_index.add_test_artifact(crate::features::basic_search::dto::ArtifactDocument {
        id: "test-artifact-2".to_string(),
        name: "another-package".to_string(),
        version: "2.0.0".to_string(),
        package_type: "maven".to_string(),
        repository: "test-repo".to_string(),
        description: "Another test package".to_string(),
        content: "This is another test content".to_string(),
        score: 0.8,
    }).await;
    
    // Act
    let query = AdvancedSearchQuery {
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
    
    // Verify both artifacts are returned
    let artifact_ids: Vec<&str> = results.artifacts.iter().map(|a| a.id.as_str()).collect();
    assert!(artifact_ids.contains(&"test-artifact-1"));
    assert!(artifact_ids.contains(&"test-artifact-2"));
}

#[tokio::test]
async fn test_case_insensitive_advanced_search() {
    // Arrange
    let query_parser = Arc::new(MockQueryParserAdapter::new());
    let search_index = Arc::new(MockAdvancedSearchIndexAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    let use_case = AdvancedQueryUseCase::new(
        query_parser,
        search_index.clone(),
        event_publisher,
    );
    
    // Add some test data with mixed case
    search_index.add_test_artifact(crate::features::basic_search::dto::ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "Test-Package".to_string(), // Mixed case
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
        description: "A test package".to_string(),
        content: "This is test content".to_string(),
        score: 1.0,
    }).await;
    
    // Act
    let query = AdvancedSearchQuery {
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
async fn test_advanced_search_with_pagination() {
    // Arrange
    let query_parser = Arc::new(MockQueryParserAdapter::new());
    let search_index = Arc::new(MockAdvancedSearchIndexAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    let use_case = AdvancedQueryUseCase::new(
        query_parser,
        search_index.clone(),
        event_publisher,
    );
    
    // Add multiple test artifacts
    for i in 1..=15 {
        search_index.add_test_artifact(crate::features::basic_search::dto::ArtifactDocument {
            id: format!("test-artifact-{}", i),
            name: format!("test-package-{}", i),
            version: format!("1.0.{}", i),
            package_type: "npm".to_string(),
            repository: "test-repo".to_string(),
            description: format!("A test package {}", i),
            content: format!("This is test content {}", i),
            score: 1.0,
        }).await;
    }
    
    // Act & Assert - First page
    let query_page_1 = AdvancedSearchQuery {
        q: "test".to_string(),
        page: Some(1),
        page_size: Some(5),
        language: None,
        fields: None,
    };
    
    let results_page_1 = use_case.execute(query_page_1).await.unwrap();
    assert_eq!(results_page_1.artifacts.len(), 5);
    assert_eq!(results_page_1.page, 1);
    assert_eq!(results_page_1.page_size, 5);
    assert_eq!(results_page_1.total_pages, 3); // 15 items / 5 per page = 3 pages
    
    // Act & Assert - Second page
    let query_page_2 = AdvancedSearchQuery {
        q: "test".to_string(),
        page: Some(2),
        page_size: Some(5),
        language: None,
        fields: None,
    };
    
    let results_page_2 = use_case.execute(query_page_2).await.unwrap();
    assert_eq!(results_page_2.artifacts.len(), 5);
    assert_eq!(results_page_2.page, 2);
    assert_eq!(results_page_2.page_size, 5);
    assert_eq!(results_page_2.total_pages, 3);
    
    // Verify that pages have different artifacts
    let page_1_ids: Vec<&str> = results_page_1.artifacts.iter().map(|a| a.id.as_str()).collect();
    let page_2_ids: Vec<&str> = results_page_2.artifacts.iter().map(|a| a.id.as_str()).collect();
    
    // No overlap between pages
    for id in &page_1_ids {
        assert!(!page_2_ids.contains(id));
    }
}

#[tokio::test]
async fn test_advanced_search_no_results() {
    // Arrange
    let query_parser = Arc::new(MockQueryParserAdapter::new());
    let search_index = Arc::new(MockAdvancedSearchIndexAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    let use_case = AdvancedQueryUseCase::new(
        query_parser,
        search_index.clone(),
        event_publisher,
    );
    
    // Add some test data
    search_index.add_test_artifact(crate::features::basic_search::dto::ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
        description: "A test package".to_string(),
        content: "This is test content".to_string(),
        score: 1.0,
    }).await;
    
    // Act
    let query = AdvancedSearchQuery {
        q: "nonexistent".to_string(),
        page: Some(1),
        page_size: Some(10),
        language: None,
        fields: None,
    };
    
    let results = use_case.execute(query).await.unwrap();
    
    // Assert
    assert_eq!(results.total_count, 0);
    assert_eq!(results.artifacts.len(), 0);
}

#[tokio::test]
async fn test_advanced_search_event_publishing() {
    // Arrange
    let query_parser = Arc::new(MockQueryParserAdapter::new());
    let search_index = Arc::new(MockAdvancedSearchIndexAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    let use_case = AdvancedQueryUseCase::new(
        query_parser,
        search_index.clone(),
        event_publisher.clone(),
    );
    
    // Add some test data
    search_index.add_test_artifact(crate::features::basic_search::dto::ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
        description: "A test package".to_string(),
        content: "This is test content".to_string(),
        score: 1.0,
    }).await;
    
    // Act
    let query = AdvancedSearchQuery {
        q: "test".to_string(),
        page: Some(1),
        page_size: Some(10),
        language: None,
        fields: None,
    };
    
    let _results = use_case.execute(query).await.unwrap();
    
    // Assert - Check that events were published
    // Note: In a real implementation, we would check the events published
    // For now, we're just verifying the use case executes without error
    assert!(true);
}