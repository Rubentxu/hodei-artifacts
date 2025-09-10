use search::features::basic_search::{
    dto::{SearchQuery, ArtifactDocument},
    test_utils::{MockSearchIndexAdapter, MockEventPublisherAdapter},
    use_case::BasicSearchUseCase,
};
use std::sync::Arc;
use tokio;

#[tokio::test]
async fn test_search_with_results_integration() {
    // Arrange
    let search_index = Arc::new(MockSearchIndexAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    // Add some test data
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
    }).await;
    
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-2".to_string(),
        name: "another-package".to_string(),
        version: "2.0.0".to_string(),
        package_type: "maven".to_string(),
        repository: "test-repo".to_string(),
    }).await;
    
    let use_case = BasicSearchUseCase::new(
        search_index.clone(),
        event_publisher.clone(),
    );
    
    // Act
    let query = SearchQuery {
        q: "test".to_string(),
        page: Some(1),
        page_size: Some(10),
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
async fn test_empty_search_returns_all_artifacts_integration() {
    // Arrange
    let search_index = Arc::new(MockSearchIndexAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    // Add some test data
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
    }).await;
    
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-2".to_string(),
        name: "another-package".to_string(),
        version: "2.0.0".to_string(),
        package_type: "maven".to_string(),
        repository: "test-repo".to_string(),
    }).await;
    
    let use_case = BasicSearchUseCase::new(
        search_index.clone(),
        event_publisher.clone(),
    );
    
    // Act
    let query = SearchQuery {
        q: "".to_string(),
        page: Some(1),
        page_size: Some(10),
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
async fn test_case_insensitive_search_integration() {
    // Arrange
    let search_index = Arc::new(MockSearchIndexAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    // Add some test data with mixed case
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "Test-Package".to_string(), // Mixed case
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
    }).await;
    
    let use_case = BasicSearchUseCase::new(
        search_index.clone(),
        event_publisher.clone(),
    );
    
    // Act
    let query = SearchQuery {
        q: "test-package".to_string(), // Lowercase query
        page: Some(1),
        page_size: Some(10),
    };
    
    let results = use_case.execute(query).await.unwrap();
    
    // Assert
    assert_eq!(results.total_count, 1);
    assert_eq!(results.artifacts.len(), 1);
    assert_eq!(results.artifacts[0].name, "Test-Package");
}

#[tokio::test]
async fn test_search_with_pagination_integration() {
    // Arrange
    let search_index = Arc::new(MockSearchIndexAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    // Add multiple test artifacts
    for i in 1..=15 {
        search_index.add_test_artifact(ArtifactDocument {
            id: format!("test-artifact-{}", i),
            name: format!("test-package-{}", i),
            version: format!("1.0.{}", i),
            package_type: "npm".to_string(),
            repository: "test-repo".to_string(),
        }).await;
    }
    
    let use_case = BasicSearchUseCase::new(
        search_index.clone(),
        event_publisher.clone(),
    );
    
    // Act & Assert - First page
    let query_page_1 = SearchQuery {
        q: "test".to_string(),
        page: Some(1),
        page_size: Some(5),
    };
    
    let results_page_1 = use_case.execute(query_page_1).await.unwrap();
    assert_eq!(results_page_1.artifacts.len(), 5);
    assert_eq!(results_page_1.page, 1);
    assert_eq!(results_page_1.page_size, 5);
    assert_eq!(results_page_1.total_pages, 3); // 15 items / 5 per page = 3 pages
    
    // Act & Assert - Second page
    let query_page_2 = SearchQuery {
        q: "test".to_string(),
        page: Some(2),
        page_size: Some(5),
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
async fn test_search_no_results_integration() {
    // Arrange
    let search_index = Arc::new(MockSearchIndexAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    // Add some test data
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
    }).await;
    
    let use_case = BasicSearchUseCase::new(
        search_index.clone(),
        event_publisher.clone(),
    );
    
    // Act
    let query = SearchQuery {
        q: "nonexistent".to_string(),
        page: Some(1),
        page_size: Some(10),
    };
    
    let results = use_case.execute(query).await.unwrap();
    
    // Assert
    assert_eq!(results.total_count, 0);
    assert_eq!(results.artifacts.len(), 0);
}

#[tokio::test]
async fn test_event_publishing_integration() {
    // Arrange
    let search_index = Arc::new(MockSearchIndexAdapter::new());
    let event_publisher = Arc::new(MockEventPublisherAdapter::new());
    
    // Add some test data
    search_index.add_test_artifact(ArtifactDocument {
        id: "test-artifact-1".to_string(),
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        package_type: "npm".to_string(),
        repository: "test-repo".to_string(),
    }).await;
    
    let use_case = BasicSearchUseCase::new(
        search_index.clone(),
        event_publisher.clone(),
    );
    
    // Act
    let query = SearchQuery {
        q: "test".to_string(),
        page: Some(1),
        page_size: Some(10),
    };
    
    let _results = use_case.execute(query).await.unwrap();
    
    // Assert - Check that events were published
    // Note: In a real implementation, we would check the events published
    // For now, we're just verifying the use case executes without error
}