use crate::application::ports::{AdvancedSearchIndex, SearchIndex};
use crate::domain::model::ArtifactSearchDocument;
use crate::error::SearchResult;
use crate::features::advanced_search::AdvancedSearchQuery;
use crate::infrastructure::tantivy_search::TantivySearchIndex;
use shared::{ArtifactId, RepositoryId, IsoTimestamp};
use std::str::FromStr;
use uuid::Uuid;

#[tokio::test]
async fn test_tantivy_search_index_initialization() -> SearchResult<()> {
    let index = TantivySearchIndex::new(None)?;
    
    // Test that search functionality works instead of accessing private fields
    let results = index.search("test", None).await?;
    assert_eq!(results.len(), 0);
    
    Ok(())
}

#[tokio::test]
async fn test_document_indexing_and_retrieval() -> SearchResult<()> {
    let index = TantivySearchIndex::new(None)?;
    
    let artifact_id = ArtifactId::from_str(&Uuid::new_v4().to_string()).unwrap();
    let repository_id = RepositoryId::from_str(&Uuid::new_v4().to_string()).unwrap();
    
    let document = ArtifactSearchDocument {
        artifact_id,
        repository_id,
        name: "test-artifact.txt".to_string(),
        version: "1.0.0".to_string(),
        description: Some("A test artifact for search".to_string()),
        tags: vec!["test".to_string(), "search".to_string()],
        indexed_at: IsoTimestamp::now(),
        relevance_score: 0.0,
    };
    
    index.index(&document).await?;
    
    let results = index.search("test", None).await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "test-artifact.txt");
    assert_eq!(results[0].artifact_id, artifact_id);
    
    Ok(())
}

#[tokio::test]
async fn test_search_with_repository_filter() -> SearchResult<()> {
    let index = TantivySearchIndex::new(None)?;
    
    let repo1_id = RepositoryId::from_str(&Uuid::new_v4().to_string()).unwrap();
    let repo2_id = RepositoryId::from_str(&Uuid::new_v4().to_string()).unwrap();
    
    let doc1 = ArtifactSearchDocument {
        artifact_id: ArtifactId::from_str(&Uuid::new_v4().to_string()).unwrap(),
        repository_id: repo1_id,
        name: "repo1-artifact.txt".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Artifact in repo1".to_string()),
        tags: vec!["repo1".to_string()],
        indexed_at: IsoTimestamp::now(),
        relevance_score: 0.0,
    };
    
    let doc2 = ArtifactSearchDocument {
        artifact_id: ArtifactId::from_str(&Uuid::new_v4().to_string()).unwrap(),
        repository_id: repo2_id,
        name: "repo2-artifact.txt".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Artifact in repo2".to_string()),
        tags: vec!["repo2".to_string()],
        indexed_at: IsoTimestamp::now(),
        relevance_score: 0.0,
    };
    
    index.index(&doc1).await?;
    index.index(&doc2).await?;
    
    let all_results = index.search("artifact", None).await?;
    assert_eq!(all_results.len(), 2);
    
    let repo1_results = index.search("artifact", Some(repo1_id.to_string())).await?;
    assert_eq!(repo1_results.len(), 1);
    assert_eq!(repo1_results[0].repository_id, repo1_id);
    
    let repo2_results = index.search("artifact", Some(repo2_id.to_string())).await?;
    assert_eq!(repo2_results.len(), 1);
    assert_eq!(repo2_results[0].repository_id, repo2_id);
    
    Ok(())
}

#[tokio::test]
async fn test_advanced_search_basic_functionality() -> SearchResult<()> {
    let index = TantivySearchIndex::new(None)?;
    
    let document = ArtifactSearchDocument {
        artifact_id: ArtifactId::from_str(&Uuid::new_v4().to_string()).unwrap(),
        repository_id: RepositoryId::from_str(&Uuid::new_v4().to_string()).unwrap(),
        name: "advanced-search-test.txt".to_string(),
        version: "2.0.0".to_string(),
        description: Some("Testing advanced search capabilities".to_string()),
        tags: vec!["advanced".to_string(), "test".to_string()],
        indexed_at: IsoTimestamp::now(),
        relevance_score: 0.0,
    };
    
    index.index(&document).await?;
    
    let query = AdvancedSearchQuery {
        q: "advanced".to_string(),
    };
    
    let result = index.advanced_search(&query).await?;
    assert_eq!(result.total, 1);
    assert!(result.hits.contains(&"advanced-search-test.txt".to_string()));
    
    Ok(())
}

#[tokio::test]
async fn test_search_multiple_fields() -> SearchResult<()> {
    let index = TantivySearchIndex::new(None)?;
    
    let doc_with_name = ArtifactSearchDocument {
        artifact_id: ArtifactId::from_str(&Uuid::new_v4().to_string()).unwrap(),
        repository_id: RepositoryId::from_str(&Uuid::new_v4().to_string()).unwrap(),
        name: "spring-boot-app".to_string(),
        version: "3.1.0".to_string(),
        description: Some("A Java application".to_string()),
        tags: vec!["java".to_string()],
        indexed_at: IsoTimestamp::now(),
        relevance_score: 0.0,
    };
    
    let doc_with_description = ArtifactSearchDocument {
        artifact_id: ArtifactId::from_str(&Uuid::new_v4().to_string()).unwrap(),
        repository_id: RepositoryId::from_str(&Uuid::new_v4().to_string()).unwrap(),
        name: "node-app".to_string(),
        version: "1.0.0".to_string(),
        description: Some("spring boot microservice".to_string()),
        tags: vec!["nodejs".to_string()],
        indexed_at: IsoTimestamp::now(),
        relevance_score: 0.0,
    };
    
    index.index(&doc_with_name).await?;
    index.index(&doc_with_description).await?;
    
    let spring_results = index.search("spring", None).await?;
    assert_eq!(spring_results.len(), 2);
    
    let java_results = index.search("java", None).await?;
    assert_eq!(java_results.len(), 1);
    
    let node_results = index.search("node", None).await?;
    assert_eq!(node_results.len(), 1);
    
    Ok(())
}

#[tokio::test]
async fn test_empty_search_returns_nothing() -> SearchResult<()> {
    let index = TantivySearchIndex::new(None)?;
    
    let results = index.search("nonexistentterm", None).await?;
    assert_eq!(results.len(), 0);
    
    Ok(())
}