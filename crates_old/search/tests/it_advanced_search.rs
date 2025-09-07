#![cfg(feature = "integration-tantivy")]

use shared_test::setup_test_environment;
use artifact::features::upload_artifact::{command::UploadArtifactCommand, handler::handle as handle_upload};
use search::application::ports::{SearchIndex, AdvancedSearchIndex};
use search::infrastructure::tantivy_search::TantivySearchIndex;
use search::features::advanced_search::{AdvancedSearchQuery, AdvancedSearchResult};
use shared::{RepositoryId, UserId};
use uuid::Uuid;

#[tokio::test]
async fn test_advanced_search_with_filters() -> Result<(), Box<dyn std::error::Error>> {
    let env = setup_test_environment(None).await;
    let search_index = TantivySearchIndex::new(None)?;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    
    // Upload multiple test artifacts with different metadata
    let test_artifacts = vec![
        ("spring-boot-app.jar", "Java Spring Boot microservice", vec!["java", "spring", "microservice"], "2.3.1"),
        ("node-api.js", "Node.js REST API service", vec!["nodejs", "javascript", "api"], "1.2.0"),
        ("python-utils.py", "Python utility functions library", vec!["python", "library", "utilities"], "0.5.0"),
        ("react-ui.zip", "React frontend application", vec!["react", "javascript", "frontend"], "3.1.4"),
        ("rust-cli", "Rust command line tool", vec!["rust", "cli", "tools"], "1.0.0"),
    ];
    
    for (file_name, description, tags, version) in test_artifacts {
        let upload_cmd = UploadArtifactCommand {
            repository_id: repository_id.clone(),
            version: artifact::domain::model::ArtifactVersion(version.to_string()),
            file_name: file_name.to_string(),
            size_bytes: 100,
            checksum: artifact::domain::model::ArtifactChecksum("d41d8cd98f00b204e9800998ecf8427e".to_string()),
            user_id: user_id.clone(),
            bytes: b"dummy content".to_vec(),
        };
        
        let upload_result = handle_upload(
            &*env.artifact_repository,
            &*env.artifact_storage,
            &*env.artifact_event_publisher,
            upload_cmd,
        ).await?;
        
        let search_document = search::domain::model::ArtifactSearchDocument {
            artifact_id: upload_result.artifact_id,
            repository_id: repository_id.clone(),
            name: file_name.to_string(),
            version: version.to_string(),
            description: Some(description.to_string()),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            indexed_at: shared::IsoTimestamp::now(),
            relevance_score: 0.0,
        };
        
        search_index.index(&search_document).await?;
    }
    
    // Test 1: Search by specific technology
    let java_query = AdvancedSearchQuery {
        q: "java".to_string(),
    };
    
    let java_results = search_index.advanced_search(&java_query).await?;
    assert_eq!(java_results.total, 1);
    assert!(java_results.hits.iter().any(|hit| hit.contains("spring-boot-app")));
    
    // Test 2: Search by category
    let api_query = AdvancedSearchQuery {
        q: "api".to_string(),
    };
    
    let api_results = search_index.advanced_search(&api_query).await?;
    assert_eq!(api_results.total, 1);
    assert!(api_results.hits.iter().any(|hit| hit.contains("node-api")));
    
    // Test 3: Search with multiple terms
    let multi_term_query = AdvancedSearchQuery {
        q: "javascript library".to_string(),
    };
    
    let multi_term_results = search_index.advanced_search(&multi_term_query).await?;
    assert_eq!(multi_term_results.total, 2); // Should find node-api.js and react-ui.zip
    
    // Test 4: Search with version filter
    let version_query = AdvancedSearchQuery {
        q: "version:1.0.0".to_string(),
    };
    
    let version_results = search_index.advanced_search(&version_query).await?;
    assert_eq!(version_results.total, 1);
    assert!(version_results.hits.iter().any(|hit| hit.contains("rust-cli")));
    
    // Test 5: Search with repository context
    let repo_query = AdvancedSearchQuery {
        q: format!("repo:{} javascript", repository_id.to_string()),
    };
    
    let repo_results = search_index.advanced_search(&repo_query).await?;
    assert_eq!(repo_results.total, 2); // node-api.js and react-ui.zip
    
    Ok(())
}

#[tokio::test]
async fn test_advanced_search_pagination() -> Result<(), Box<dyn std::error::Error>> {
    let env = setup_test_environment(None).await;
    let search_index = TantivySearchIndex::new(None)?;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    
    // Upload multiple test artifacts
    for i in 1..=15 {
        let upload_cmd = UploadArtifactCommand {
            repository_id: repository_id.clone(),
            version: artifact::domain::model::ArtifactVersion(format!("1.{}.0", i)),
            file_name: format!("test-artifact-{}.jar", i),
            size_bytes: 100,
            checksum: artifact::domain::model::ArtifactChecksum("d41d8cd98f00b204e9800998ecf8427e".to_string()),
            user_id: user_id.clone(),
            bytes: b"dummy content".to_vec(),
        };
        
        let upload_result = handle_upload(
            &*env.artifact_repository,
            &*env.artifact_storage,
            &*env.artifact_event_publisher,
            upload_cmd,
        ).await?;
        
        let search_document = search::domain::model::ArtifactSearchDocument {
            artifact_id: upload_result.artifact_id,
            repository_id: repository_id.clone(),
            name: format!("test-artifact-{}", i),
            version: format!("1.{}.0", i),
            description: Some(format!("Test artifact number {}", i)),
            tags: vec!["test".to_string(), "artifact".to_string()],
            indexed_at: shared::IsoTimestamp::now(),
            relevance_score: 0.0,
        };
        
        search_index.index(&search_document).await?;
    }
    
    // Test pagination with limit and offset
    let query = AdvancedSearchQuery {
        q: "test".to_string(),
    };
    
    // First page: 10 results
    let page1_results = search_index.advanced_search_with_pagination(&query, 0, 10).await?;
    assert_eq!(page1_results.total, 15);
    assert_eq!(page1_results.hits.len(), 10);
    
    // Second page: remaining 5 results
    let page2_results = search_index.advanced_search_with_pagination(&query, 10, 10).await?;
    assert_eq!(page2_results.total, 15);
    assert_eq!(page2_results.hits.len(), 5);
    
    // Verify no overlap between pages
    let page1_names: Vec<String> = page1_results.hits.iter().map(|s| s.clone()).collect();
    let page2_names: Vec<String> = page2_results.hits.iter().map(|s| s.clone()).collect();
    
    for name in &page1_names {
        assert!(!page2_names.contains(name), "Overlap found between pages: {}", name);
    }
    
    Ok(())
}

#[tokio::test]
async fn test_advanced_search_facets() -> Result<(), Box<dyn std::error::Error>> {
    let env = setup_test_environment(None).await;
    let search_index = TantivySearchIndex::new(None)?;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    
    // Upload artifacts with different tags for facet testing
    let facet_artifacts = vec![
        ("java-service.jar", vec!["java", "backend", "microservice"]),
        ("node-service.js", vec!["nodejs", "backend", "api"]),
        ("react-app.zip", vec!["react", "frontend", "ui"]),
        ("python-tool.py", vec!["python", "cli", "tools"]),
        ("rust-library", vec!["rust", "library", "performance"]),
    ];
    
    for (file_name, tags) in facet_artifacts {
        let upload_cmd = UploadArtifactCommand {
            repository_id: repository_id.clone(),
            version: artifact::domain::model::ArtifactVersion("1.0.0".to_string()),
            file_name: file_name.to_string(),
            size_bytes: 100,
            checksum: artifact::domain::model::ArtifactChecksum("d41d8cd98f00b204e9800998ecf8427e".to_string()),
            user_id: user_id.clone(),
            bytes: b"dummy content".to_vec(),
        };
        
        let upload_result = handle_upload(
            &*env.artifact_repository,
            &*env.artifact_storage,
            &*env.artifact_event_publisher,
            upload_cmd,
        ).await?;
        
        let search_document = search::domain::model::ArtifactSearchDocument {
            artifact_id: upload_result.artifact_id,
            repository_id: repository_id.clone(),
            name: file_name.to_string(),
            version: "1.0.0".to_string(),
            description: Some(format!("{} artifact", file_name)),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            indexed_at: shared::IsoTimestamp::now(),
            relevance_score: 0.0,
        };
        
        search_index.index(&search_document).await?;
    }
    
    // Test facet aggregation
    let facet_query = AdvancedSearchQuery {
        q: "*".to_string(), // Match all
    };
    
    let facet_results = search_index.advanced_search_with_facets(&facet_query, vec!["tags".to_string()]).await?;
    
    // Verify facet counts
    assert!(facet_results.facets.contains_key("tags"));
    let tag_facets = &facet_results.facets["tags"];
    
    // Should have counts for each tag
    assert!(tag_facets.get("backend").unwrap_or(&0) >= &2);
    assert!(tag_facets.get("frontend").unwrap_or(&0) >= &1);
    assert!(tag_facets.get("java").unwrap_or(&0) >= &1);
    assert!(tag_facets.get("nodejs").unwrap_or(&0) >= &1);
    
    Ok(())
}

#[tokio::test]
async fn test_advanced_search_relevance_scoring() -> Result<(), Box<dyn std::error::Error>> {
    let env = setup_test_environment(None).await;
    let search_index = TantivySearchIndex::new(None)?;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    
    // Upload artifacts with different relevance characteristics
    let relevance_artifacts = vec![
        ("spring-boot-microservice.jar", "Spring Boot microservice for backend development", vec!["java", "spring", "microservice"]),
        ("node-express-api.js", "Express.js API server for Node.js applications", vec!["nodejs", "express", "api"]),
        ("react-frontend-app.zip", "React application for modern web frontend", vec!["react", "frontend", "javascript"]),
    ];
    
    for (file_name, description, tags) in relevance_artifacts {
        let upload_cmd = UploadArtifactCommand {
            repository_id: repository_id.clone(),
            version: artifact::domain::model::ArtifactVersion("1.0.0".to_string()),
            file_name: file_name.to_string(),
            size_bytes: 100,
            checksum: artifact::domain::model::ArtifactChecksum("d41d8cd98f00b204e9800998ecf8427e".to_string()),
            user_id: user_id.clone(),
            bytes: b"dummy content".to_vec(),
        };
        
        let upload_result = handle_upload(
            &*env.artifact_repository,
            &*env.artifact_storage,
            &*env.artifact_event_publisher,
            upload_cmd,
        ).await?;
        
        let search_document = search::domain::model::ArtifactSearchDocument {
            artifact_id: upload_result.artifact_id,
            repository_id: repository_id.clone(),
            name: file_name.to_string(),
            version: "1.0.0".to_string(),
            description: Some(description.to_string()), // Rich description for better relevance
            tags: tags.iter().map(|s| s.to_string()).collect(),
            indexed_at: shared::IsoTimestamp::now(),
            relevance_score: 0.0,
        };
        
        search_index.index(&search_document).await?;
    }
    
    // Test relevance-based search
    let relevance_query = AdvancedSearchQuery {
        q: "microservice api development".to_string(),
    };
    
    let relevance_results = search_index.advanced_search_with_relevance(&relevance_query).await?;
    
    // Should return results ordered by relevance
    assert!(relevance_results.total >= 2);
    
    // The first result should be the most relevant
    let first_result = &relevance_results.hits[0];
    assert!(first_result.contains("spring-boot") || first_result.contains("node-express"));
    
    Ok(())
}