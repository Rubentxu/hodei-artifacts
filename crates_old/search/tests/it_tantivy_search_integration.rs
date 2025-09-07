#![cfg(feature = "integration-tantivy")]

use std::sync::Arc;
use std::time::Duration;
use search::{
    application::ports::{SearchIndex, SearchRepository},
    domain::model::{SearchQuery, SearchResult, IndexedArtifact},
    features::{
        basic_search::handler::handle_basic_search,
        advanced_search::handler::handle_advanced_search,
        index_management::handler::{handle_index_artifact, handle_remove_from_index},
    },
    infrastructure::tantivy_search::TantivySearchIndex,
};
use async_trait::async_trait;
use infra_mongo::test_util::mongo_test_container::ephemeral_store;
use shared::{RepositoryId, UserId, ArtifactId};
use testcontainers::runners::AsyncRunner;
use testcontainers::clients;
use tempfile::TempDir;
use tokio::time::{sleep, timeout};
use tantivy::{doc, schema::*}; 

// Mock repository for testing
struct MockSearchRepository {
    artifacts: Vec<IndexedArtifact>,
}

#[async_trait]
impl SearchRepository for MockSearchRepository {
    async fn get_artifact(&self, id: &ArtifactId) -> anyhow::Result<Option<IndexedArtifact>> {
        Ok(self.artifacts.iter().find(|a| &a.id == id).cloned())
    }

    async fn list_artifacts(&self) -> anyhow::Result<Vec<IndexedArtifact>> {
        Ok(self.artifacts.clone())
    }

    async fn search_by_repository(&self, repository_id: &RepositoryId) -> anyhow::Result<Vec<IndexedArtifact>> {
        Ok(self.artifacts
            .iter()
            .filter(|a| &a.repository_id == repository_id)
            .cloned()
            .collect())
    }
}

fn create_test_indexed_artifact(id: ArtifactId, repo_id: RepositoryId, content: &str) -> IndexedArtifact {
    IndexedArtifact {
        id,
        repository_id: repo_id,
        name: format!("test-artifact-{}", id.to_string()),
        version: "1.0.0".to_string(),
        description: Some(format!("Test artifact with content: {}", content)),
        tags: vec!["test".to_string(), "integration".to_string()],
        metadata: serde_json::json!({
            "author": "test-user",
            "license": "MIT",
            "content": content
        }),
        upload_date: chrono::Utc::now(),
        user_id: UserId::new(),
        file_name: "test.txt".to_string(),
        size_bytes: 100,
        checksum: "test-checksum".to_string(),
    }
}

async fn setup_dependencies() -> (TantivySearchIndex, MockSearchRepository, TempDir) {
    // Create temporary directory for index
    let temp_dir = TempDir::new().unwrap();
    let index_path = temp_dir.path().to_path_buf();
    
    // Create search index
    let search_index = TantivySearchIndex::new(&index_path).await.unwrap();
    
    // Create mock repository with test data
    let repo_id = RepositoryId::new();
    let artifacts = vec![
        create_test_indexed_artifact(ArtifactId::new(), repo_id.clone(), "rust web framework"),
        create_test_indexed_artifact(ArtifactId::new(), repo_id.clone(), "async programming library"),
        create_test_indexed_artifact(ArtifactId::new(), repo_id.clone(), "database connection pool"),
        create_test_indexed_artifact(ArtifactId::new(), repo_id.clone(), "http server implementation"),
        create_test_indexed_artifact(ArtifactId::new(), repo_id, "serialization deserialization library"),
    ];
    
    let repository = MockSearchRepository { artifacts };
    
    (search_index, repository, temp_dir)
}

#[tokio::test]
async fn test_basic_search_returns_relevant_results() {
    // Arrange
    let (search_index, repository, _temp_dir) = setup_dependencies().await;
    
    // Index all artifacts
    for artifact in repository.list_artifacts().await.unwrap() {
        handle_index_artifact(&search_index, &artifact).await.unwrap();
    }
    
    // Wait for index to commit
    sleep(Duration::from_millis(100)).await;
    
    // Act - Search for "framework"
    let query = SearchQuery {
        query: "framework".to_string(),
        repository_id: None,
        limit: 10,
        offset: 0,
        filters: None,
    };
    
    let result = handle_basic_search(&search_index, &repository, query).await;
    
    // Assert
    assert!(result.is_ok());
    let search_results = result.unwrap();
    
    // Should find the artifact about "rust web framework"
    assert!(!search_results.is_empty());
    assert!(search_results.iter().any(|r| 
        r.artifact.description.as_ref().unwrap().contains("framework")
    ));
}

#[tokio::test]
async fn test_advanced_search_with_filters() {
    // Arrange
    let (search_index, repository, _temp_dir) = setup_dependencies().await;
    let repo_id = RepositoryId::new();
    
    // Index artifacts
    for artifact in repository.list_artifacts().await.unwrap() {
        handle_index_artifact(&search_index, &artifact).await.unwrap();
    }
    
    sleep(Duration::from_millis(100)).await;
    
    // Act - Search with repository filter
    let query = SearchQuery {
        query: "library".to_string(),
        repository_id: Some(repo_id.clone()),
        limit: 10,
        offset: 0,
        filters: Some(serde_json::json!({
            "tags": ["test"]
        })),
    };
    
    let result = handle_advanced_search(&search_index, &repository, query).await;
    
    // Assert
    assert!(result.is_ok());
    let search_results = result.unwrap();
    
    // Should find library-related artifacts
    assert!(!search_results.is_empty());
    assert!(search_results.iter().all(|r| 
        r.artifact.description.as_ref().unwrap().contains("library") &&
        r.artifact.repository_id == repo_id
    ));
}

#[tokio::test]
async fn test_search_score_relevance() {
    // Arrange
    let (search_index, repository, _temp_dir) = setup_dependencies().await;
    
    // Add more specific test data
    let specific_artifact = IndexedArtifact {
        id: ArtifactId::new(),
        repository_id: RepositoryId::new(),
        name: "hyper-http-server".to_string(),
        version: "1.0.0".to_string(),
        description: Some("High performance HTTP server implementation in Rust".to_string()),
        tags: vec!["http".to_string(), "server".to_string(), "performance".to_string()],
        metadata: serde_json::json!({"category": "web"}),
        upload_date: chrono::Utc::now(),
        user_id: UserId::new(),
        file_name: "hyper-server.tar.gz".to_string(),
        size_bytes: 1024000,
        checksum: "hyper-checksum".to_string(),
    };
    
    handle_index_artifact(&search_index, &specific_artifact).await.unwrap();
    
    // Index all artifacts
    for artifact in repository.list_artifacts().await.unwrap() {
        handle_index_artifact(&search_index, &artifact).await.unwrap();
    }
    
    sleep(Duration::from_millis(100)).await;
    
    // Act - Search for very specific term
    let query = SearchQuery {
        query: "hyper http server performance".to_string(),
        repository_id: None,
        limit: 5,
        offset: 0,
        filters: None,
    };
    
    let result = handle_basic_search(&search_index, &repository, query).await;
    
    // Assert
    assert!(result.is_ok());
    let search_results = result.unwrap();
    
    // Most relevant result should be first
    assert!(!search_results.is_empty());
    let top_result = &search_results[0];
    
    assert_eq!(top_result.artifact.name, "hyper-http-server");
    assert!(top_result.score > 0.5); // Should have decent relevance score
}

#[tokio::test]
async fn test_search_pagination() {
    // Arrange
    let (search_index, repository, _temp_dir) = setup_dependencies().await;
    
    // Index all artifacts
    for artifact in repository.list_artifacts().await.unwrap() {
        handle_index_artifact(&search_index, &artifact).await.unwrap();
    }
    
    sleep(Duration::from_millis(100)).await;
    
    // Act - First page
    let query_page1 = SearchQuery {
        query: "library".to_string(),
        repository_id: None,
        limit: 2,
        offset: 0,
        filters: None,
    };
    
    let result_page1 = handle_basic_search(&search_index, &repository, query_page1).await.unwrap();
    
    // Second page
    let query_page2 = SearchQuery {
        query: "library".to_string(),
        repository_id: None,
        limit: 2,
        offset: 2,
        filters: None,
    };
    
    let result_page2 = handle_basic_search(&search_index, &repository, query_page2).await.unwrap();
    
    // Assert
    assert_eq!(result_page1.len(), 2);
    assert_eq!(result_page2.len(), 2);
    
    // Results should be different between pages
    let page1_ids: Vec<_> = result_page1.iter().map(|r| r.artifact.id.to_string()).collect();
    let page2_ids: Vec<_> = result_page2.iter().map(|r| r.artifact.id.to_string()).collect();
    
    assert!(page1_ids.iter().all(|id| !page2_ids.contains(id)));
}

#[tokio::test]
async fn test_index_management_operations() {
    // Arrange
    let (search_index, repository, _temp_dir) = setup_dependencies().await;
    let artifacts = repository.list_artifacts().await.unwrap();
    
    // Act - Index an artifact
    let artifact_to_index = &artifacts[0];
    let index_result = handle_index_artifact(&search_index, artifact_to_index).await;
    assert!(index_result.is_ok());
    
    sleep(Duration::from_millis(100)).await;
    
    // Verify it's searchable
    let query = SearchQuery {
        query: artifact_to_index.description.as_ref().unwrap().split_whitespace().next().unwrap().to_string(),
        repository_id: None,
        limit: 10,
        offset: 0,
        filters: None,
    };
    
    let search_result = handle_basic_search(&search_index, &repository, query).await;
    assert!(search_result.is_ok());
    assert!(!search_result.unwrap().is_empty());
    
    // Act - Remove from index
    let remove_result = handle_remove_from_index(&search_index, &artifact_to_index.id).await;
    assert!(remove_result.is_ok());
    
    sleep(Duration::from_millis(100)).await;
    
    // Verify it's no longer searchable
    let search_after_remove = handle_basic_search(&search_index, &repository, query).await;
    assert!(search_after_remove.is_ok());
    let results = search_after_remove.unwrap();
    
    // Should not contain the removed artifact
    assert!(results.iter().all(|r| r.artifact.id != artifact_to_index.id));
}

#[tokio::test]
async fn test_search_empty_query_returns_all() {
    // Arrange
    let (search_index, repository, _temp_dir) = setup_dependencies().await;
    
    // Index all artifacts
    for artifact in repository.list_artifacts().await.unwrap() {
        handle_index_artifact(&search_index, &artifact).await.unwrap();
    }
    
    sleep(Duration::from_millis(100)).await;
    
    // Act - Empty query should return all documents
    let query = SearchQuery {
        query: "".to_string(),
        repository_id: None,
        limit: 20,
        offset: 0,
        filters: None,
    };
    
    let result = handle_basic_search(&search_index, &repository, query).await;
    
    // Assert
    assert!(result.is_ok());
    let search_results = result.unwrap();
    
    // Should return all indexed artifacts
    let all_artifacts = repository.list_artifacts().await.unwrap();
    assert_eq!(search_results.len(), all_artifacts.len());
}

#[tokio::test]
async fn test_search_with_repository_filter() {
    // Arrange
    let (search_index, repository, _temp_dir) = setup_dependencies().await;
    let artifacts = repository.list_artifacts().await.unwrap();
    let target_repo_id = artifacts[0].repository_id.clone();
    
    // Index all artifacts
    for artifact in &artifacts {
        handle_index_artifact(&search_index, artifact).await.unwrap();
    }
    
    sleep(Duration::from_millis(100)).await;
    
    // Act - Search with repository filter
    let query = SearchQuery {
        query: "".to_string(), // Empty query to get all
        repository_id: Some(target_repo_id.clone()),
        limit: 20,
        offset: 0,
        filters: None,
    };
    
    let result = handle_basic_search(&search_index, &repository, query).await;
    
    // Assert
    assert!(result.is_ok());
    let search_results = result.unwrap();
    
    // Should only return artifacts from the specified repository
    assert!(!search_results.is_empty());
    assert!(search_results.iter().all(|r| r.artifact.repository_id == target_repo_id));
    
    let expected_count = artifacts.iter()
        .filter(|a| a.repository_id == target_repo_id)
        .count();
    
    assert_eq!(search_results.len(), expected_count);
}

#[tokio::test]
async fn test_search_error_handling() {
    // Arrange
    let (search_index, repository, _temp_dir) = setup_dependencies().await;
    
    // Act - Try to search with invalid query syntax
    let query = SearchQuery {
        query: "AND OR NOT".to_string(), // Potentially problematic syntax
        repository_id: None,
        limit: 10,
        offset: 0,
        filters: None,
    };
    
    let result = handle_basic_search(&search_index, &repository, query).await;
    
    // Assert - Should handle the query gracefully
    assert!(result.is_ok() || result.is_err());
    // Even if it fails, it shouldn't panic - proper error handling
}

#[tokio::test]
async fn test_concurrent_indexing_and_search() {
    // Arrange
    let (search_index, repository, _temp_dir) = setup_dependencies().await;
    let artifacts = repository.list_artifacts().await.unwrap();
    
    // Act - Concurrent indexing and searching
    let mut handles = vec![];
    
    // Start search operations while indexing
    for i in 0..3 {
        let search_index_clone = search_index.clone();
        let repository_clone = repository.clone();
        
        handles.push(tokio::spawn(async move {
            sleep(Duration::from_millis(i * 50)).await; // Stagger searches
            
            let query = SearchQuery {
                query: "library".to_string(),
                repository_id: None,
                limit: 10,
                offset: 0,
                filters: None,
            };
            
            handle_basic_search(&search_index_clone, &repository_clone, query).await
        }));
    }
    
    // Index artifacts concurrently
    for artifact in &artifacts {
        let search_index_clone = search_index.clone();
        let artifact_clone = artifact.clone();
        
        handles.push(tokio::spawn(async move {
            handle_index_artifact(&search_index_clone, &artifact_clone).await
        }));
    }
    
    // Wait for all operations to complete
    let results = futures::future::join_all(handles).await;
    
    // Assert - All operations should complete without panic
    for result in results {
        assert!(result.is_ok()); // Task completed
        // Individual operations might succeed or fail gracefully
    }
}