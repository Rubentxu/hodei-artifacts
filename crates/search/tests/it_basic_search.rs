use shared_test::setup_test_environment;
use artifact::features::upload_artifact::{command::UploadArtifactCommand, handler::handle as handle_upload};
use search::application::ports::{SearchIndex, AdvancedSearchIndex};
use search::infrastructure::tantivy_search::TantivySearchIndex;
use shared::{RepositoryId, UserId};
use uuid::Uuid;

#[tokio::test]
async fn test_basic_search_e2e() -> Result<(), Box<dyn std::error::Error>> {
    // Setup test environment with Docker Compose
    let env = setup_test_environment(None).await;
    
    // Create search index
    let search_index = TantivySearchIndex::new(None)?;
    
    // Create test data
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    
    // Upload test artifact
    let upload_cmd = UploadArtifactCommand {
        repository_id: repository_id.clone(),
        version: artifact::domain::model::ArtifactVersion("1.0.0".to_string()),
        file_name: "search-test-file.txt".to_string(),
        size_bytes: 28,
        checksum: artifact::domain::model::ArtifactChecksum("d41d8cd98f00b204e9800998ecf8427e".to_string()),
        user_id: user_id.clone(),
        bytes: b"This is a test file for search.".to_vec(),
    };
    
    let upload_result = handle_upload(
        &*env.artifact_repository,
        &*env.artifact_storage,
        &*env.artifact_event_publisher,
        upload_cmd,
    ).await?;
    
    // Create search document for indexing
    let search_document = search::domain::model::ArtifactSearchDocument {
        artifact_id: upload_result.artifact_id,
        repository_id: repository_id.clone(),
        name: "search-test-file.txt".to_string(),
        version: "1.0.0".to_string(),
        description: Some("A test file for search functionality".to_string()),
        tags: vec!["test".to_string(), "search".to_string(), "integration".to_string()],
        indexed_at: shared::IsoTimestamp::now(),
        relevance_score: 0.0,
    };
    
    // Index the document
    search_index.index(&search_document).await?;
    
    // Test basic search
    let results = search_index.search("test", None).await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "search-test-file.txt");
    assert_eq!(results[0].repository_id, repository_id);
    
    // Test repository-filtered search
    let filtered_results = search_index.search("test", Some(repository_id.to_string())).await?;
    assert_eq!(filtered_results.len(), 1);
    
    // Test search with non-existent term
    let empty_results = search_index.search("nonexistent", None).await?;
    assert_eq!(empty_results.len(), 0);
    
    Ok(())
}

#[tokio::test]
async fn test_search_multiple_artifacts() -> Result<(), Box<dyn std::error::Error>> {
    let env = setup_test_environment(None).await;
    let search_index = TantivySearchIndex::new(None)?;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    
    // Upload multiple artifacts with different content
    let artifacts = vec![
        ("spring-boot-app.jar", "Java Spring Boot application", vec!["java", "spring"]),
        ("node-app.js", "Node.js microservice", vec!["nodejs", "javascript"]),
        ("python-lib.py", "Python utility library", vec!["python", "library"]),
    ];
    
    for (file_name, description, tags) in artifacts {
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
            description: Some(description.to_string()),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            indexed_at: shared::IsoTimestamp::now(),
            relevance_score: 0.0,
        };
        
        search_index.index(&search_document).await?;
    }
    
    // Test search across multiple fields
    let java_results = search_index.search("java", None).await?;
    println!("Java search results: {:?}", java_results.iter().map(|r| &r.name).collect::<Vec<_>>());
    assert_eq!(java_results.len(), 1);
    assert_eq!(java_results[0].name, "spring-boot-app.jar");
    
    let node_results = search_index.search("nodejs", None).await?;
    assert_eq!(node_results.len(), 1);
    assert_eq!(node_results[0].name, "node-app.js");
    
    let all_results = search_index.search("app", None).await?;
    assert_eq!(all_results.len(), 2); // spring-boot-app and node-app
    
    Ok(())
}

#[tokio::test]
async fn test_advanced_search_functionality() -> Result<(), Box<dyn std::error::Error>> {
    let env = setup_test_environment(None).await;
    let search_index = TantivySearchIndex::new(None)?;
    
    let repository_id = RepositoryId::new();
    let user_id = UserId::new();
    
    // Upload test artifact
    let upload_cmd = UploadArtifactCommand {
        repository_id: repository_id.clone(),
        version: artifact::domain::model::ArtifactVersion("2.1.0".to_string()),
        file_name: "advanced-search-test.jar".to_string(),
        size_bytes: 200,
        checksum: artifact::domain::model::ArtifactChecksum("d41d8cd98f00b204e9800998ecf8427e".to_string()),
        user_id: user_id.clone(),
        bytes: b"advanced test content".to_vec(),
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
        name: "advanced-search-test.jar".to_string(),
        version: "2.1.0".to_string(),
        description: Some("Advanced search functionality test artifact".to_string()),
        tags: vec!["advanced".to_string(), "test".to_string(), "search".to_string()],
        indexed_at: shared::IsoTimestamp::now(),
        relevance_score: 0.0,
    };
    
    search_index.index(&search_document).await?;
    
    // Test advanced search
    let advanced_query = search::features::advanced_search::AdvancedSearchQuery {
        q: "advanced functionality".to_string(),
    };
    
    let advanced_results = search_index.advanced_search(&advanced_query).await?;
    assert_eq!(advanced_results.total, 1);
    assert!(advanced_results.hits.contains(&"advanced-search-test.jar".to_string()));
    
    Ok(())
}
