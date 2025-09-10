use std::sync::Arc;
use tokio;

use crate::features::full_text_search::{
    dto::{FullTextSearchQuery, FullTextSearchResults, IndexedArtifact, ArtifactMetadata},
    test_utils::{MockSearchEngineAdapter, MockIndexerAdapter},
    use_case::FullTextSearchUseCase,
    ports::{SearchEnginePort, IndexerPort},
    error::FullTextSearchError,
};

#[tokio::test]
async fn test_full_text_search_with_results() {
    // Arrange
    let search_engine = Arc::new(MockSearchEngineAdapter::new());
    let indexer = Arc::new(MockIndexerAdapter::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_engine.clone() as Arc<dyn SearchEnginePort>,
        indexer.clone() as Arc<dyn IndexerPort>,
    );
    
    // Act
    let query = FullTextSearchQuery {
        q: "test".to_string(),
        page: Some(1),
        page_size: Some(20),
        language: None,
        fields: None,
    };
    
    let results = use_case.execute(query).await.unwrap();
    
    // Assert
    assert_eq!(results.total_count, 0);
    assert_eq!(results.artifacts.len(), 0);
    assert_eq!(results.page, 1);
    assert_eq!(results.page_size, 20);
}

#[tokio::test]
async fn test_full_text_search_empty_query() {
    // Arrange
    let search_engine = Arc::new(MockSearchEngineAdapter::new());
    let indexer = Arc::new(MockIndexerAdapter::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_engine.clone() as Arc<dyn SearchEnginePort>,
        indexer.clone() as Arc<dyn IndexerPort>,
    );
    
    // Act
    let query = FullTextSearchQuery {
        q: "".to_string(),
        page: Some(1),
        page_size: Some(20),
        language: None,
        fields: None,
    };
    
    let result = use_case.execute(query).await;
    
    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        FullTextSearchError::InvalidQueryError(_) => {},
        _ => panic!("Expected InvalidQueryError"),
    }
}

#[tokio::test]
async fn test_full_text_search_whitespace_only_query() {
    // Arrange
    let search_engine = Arc::new(MockSearchEngineAdapter::new());
    let indexer = Arc::new(MockIndexerAdapter::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_engine.clone() as Arc<dyn SearchEnginePort>,
        indexer.clone() as Arc<dyn IndexerPort>,
    );
    
    // Act
    let query = FullTextSearchQuery {
        q: "   ".to_string(),
        page: Some(1),
        page_size: Some(20),
        language: None,
        fields: None,
    };
    
    let result = use_case.execute(query).await;
    
    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        FullTextSearchError::InvalidQueryError(_) => {},
        _ => panic!("Expected InvalidQueryError"),
    }
}

#[tokio::test]
async fn test_index_single_artifact() {
    // Arrange
    let search_engine = Arc::new(MockSearchEngineAdapter::new());
    let indexer = Arc::new(MockIndexerAdapter::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_engine.clone() as Arc<dyn SearchEnginePort>,
        indexer.clone() as Arc<dyn IndexerPort>,
    );
    
    let artifact = IndexedArtifact {
        id: "test-artifact-1".to_string(),
        content: "This is test content".to_string(),
        metadata: ArtifactMetadata {
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
            description: "A test package".to_string(),
            package_type: "npm".to_string(),
            repository: "test-repo".to_string(),
            tags: vec!["test".to_string(), "package".to_string()],
            authors: vec!["test-author".to_string()],
            licenses: vec!["MIT".to_string()],
            keywords: vec!["test".to_string(), "example".to_string()],
        },
        language: "en".to_string(),
        indexed_at: chrono::Utc::now(),
    };
    
    // Act
    let result = use_case.index_artifact(artifact.clone()).await;
    
    // Assert
    assert!(result.is_ok());
    
    // Verify the artifact was indexed
    let indexed_artifacts = indexer.get_indexed_artifacts();
    assert_eq!(indexed_artifacts.len(), 1);
    assert_eq!(indexed_artifacts[0].id, artifact.id);
    assert_eq!(indexed_artifacts[0].content, artifact.content);
}

#[tokio::test]
async fn test_index_batch_artifacts() {
    // Arrange
    let search_engine = Arc::new(MockSearchEngineAdapter::new());
    let indexer = Arc::new(MockIndexerAdapter::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_engine.clone() as Arc<dyn SearchEnginePort>,
        indexer.clone() as Arc<dyn IndexerPort>,
    );
    
    let artifacts = vec![
        IndexedArtifact {
            id: "test-artifact-1".to_string(),
            content: "This is test content 1".to_string(),
            metadata: ArtifactMetadata {
                name: "test-package-1".to_string(),
                version: "1.0.0".to_string(),
                description: "A test package 1".to_string(),
                package_type: "npm".to_string(),
                repository: "test-repo".to_string(),
                tags: vec!["test".to_string(), "package".to_string()],
                authors: vec!["test-author".to_string()],
                licenses: vec!["MIT".to_string()],
                keywords: vec!["test".to_string(), "example".to_string()],
            },
            language: "en".to_string(),
            indexed_at: chrono::Utc::now(),
        },
        IndexedArtifact {
            id: "test-artifact-2".to_string(),
            content: "This is test content 2".to_string(),
            metadata: ArtifactMetadata {
                name: "test-package-2".to_string(),
                version: "2.0.0".to_string(),
                description: "A test package 2".to_string(),
                package_type: "maven".to_string(),
                repository: "test-repo".to_string(),
                tags: vec!["test".to_string(), "package".to_string()],
                authors: vec!["test-author".to_string()],
                licenses: vec!["Apache-2.0".to_string()],
                keywords: vec!["test".to_string(), "example".to_string()],
            },
            language: "en".to_string(),
            indexed_at: chrono::Utc::now(),
        },
    ];
    
    // Act
    let result = use_case.index_artifacts_batch(artifacts.clone()).await;
    
    // Assert
    assert!(result.is_ok());
    
    // Verify the artifacts were indexed
    let indexed_artifacts = indexer.get_indexed_artifacts();
    assert_eq!(indexed_artifacts.len(), 2);
    assert_eq!(indexed_artifacts[0].id, artifacts[0].id);
    assert_eq!(indexed_artifacts[1].id, artifacts[1].id);
}

#[tokio::test]
async fn test_index_empty_batch_artifacts() {
    // Arrange
    let search_engine = Arc::new(MockSearchEngineAdapter::new());
    let indexer = Arc::new(MockIndexerAdapter::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_engine.clone() as Arc<dyn SearchEnginePort>,
        indexer.clone() as Arc<dyn IndexerPort>,
    );
    
    let artifacts = vec![];
    
    // Act
    let result = use_case.index_artifacts_batch(artifacts).await;
    
    // Assert
    assert!(result.is_ok());
    
    // Verify no artifacts were indexed
    let indexed_artifacts = indexer.get_indexed_artifacts();
    assert_eq!(indexed_artifacts.len(), 0);
}

#[tokio::test]
async fn test_get_suggestions() {
    // Arrange
    let search_engine = Arc::new(MockSearchEngineAdapter::new());
    let indexer = Arc::new(MockIndexerAdapter::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_engine.clone() as Arc<dyn SearchEnginePort>,
        indexer.clone() as Arc<dyn IndexerPort>,
    );
    
    // Act
    let result = use_case.get_suggestions("test", 10).await;
    
    // Assert
    assert!(result.is_ok());
    let suggestions = result.unwrap();
    assert_eq!(suggestions.len(), 0); // Mock returns empty suggestions
}

#[tokio::test]
async fn test_get_stats() {
    // Arrange
    let search_engine = Arc::new(MockSearchEngineAdapter::new());
    let indexer = Arc::new(MockIndexerAdapter::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_engine.clone() as Arc<dyn SearchEnginePort>,
        indexer.clone() as Arc<dyn IndexerPort>,
    );
    
    // Act
    let result = use_case.get_stats().await;
    
    // Assert
    assert!(result.is_ok());
    let stats = result.unwrap();
    assert_eq!(stats.total_documents, 0); // Mock returns zero stats
}

#[tokio::test]
async fn test_search_engine_failure() {
    // Arrange
    let search_engine = Arc::new(MockSearchEngineAdapter::new().with_should_fail(true));
    let indexer = Arc::new(MockIndexerAdapter::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_engine.clone() as Arc<dyn SearchEnginePort>,
        indexer.clone() as Arc<dyn IndexerPort>,
    );
    
    // Act
    let query = FullTextSearchQuery {
        q: "test".to_string(),
        page: Some(1),
        page_size: Some(20),
        language: None,
        fields: None,
    };
    
    let result = use_case.execute(query).await;
    
    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        FullTextSearchError::SearchError(_) => {},
        _ => panic!("Expected SearchError"),
    }
}

#[tokio::test]
async fn test_indexer_failure() {
    // Arrange
    let search_engine = Arc::new(MockSearchEngineAdapter::new());
    let indexer = Arc::new(MockIndexerAdapter::new().with_should_fail(true));
    
    let use_case = FullTextSearchUseCase::new(
        search_engine.clone() as Arc<dyn SearchEnginePort>,
        indexer.clone() as Arc<dyn IndexerPort>,
    );
    
    let artifact = IndexedArtifact {
        id: "test-artifact-1".to_string(),
        content: "This is test content".to_string(),
        metadata: ArtifactMetadata {
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
            description: "A test package".to_string(),
            package_type: "npm".to_string(),
            repository: "test-repo".to_string(),
            tags: vec!["test".to_string(), "package".to_string()],
            authors: vec!["test-author".to_string()],
            licenses: vec!["MIT".to_string()],
            keywords: vec!["test".to_string(), "example".to_string()],
        },
        language: "en".to_string(),
        indexed_at: chrono::Utc::now(),
    };
    
    // Act
    let result = use_case.index_artifact(artifact).await;
    
    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        FullTextSearchError::IndexingError(_) => {},
        _ => panic!("Expected IndexingError"),
    }
}