use async_trait::async_trait;
use std::sync::Arc;
use tantivy::{
    collector::TopDocs,
    query::QueryParser,
    Index, IndexReader, ReloadPolicy,
};
use tracing::{debug, info, error};

use crate::features::full_text_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    error::FullTextSearchError,
    ports::{SearchIndexPort, ArtifactRepositoryPort, EventPublisherPort},
    adapter::TantivySearchAdapter,
};

/// Tantivy-based search integration for full-text search
pub struct TantivySearchIntegration {
    index: Arc<TantivySearchAdapter>,
    index_reader: Arc<IndexReader>,
    search_index: Arc<dyn SearchIndexPort>,
    artifact_repository: Arc<dyn ArtifactRepositoryPort>,
    event_publisher: Arc<dyn EventPublisherPort>,
}

impl TantivySearchIntegration {
    pub fn new(
        index: Arc<TantivySearchAdapter>,
        search_index: Arc<dyn SearchIndexPort>,
        artifact_repository: Arc<dyn ArtifactRepositoryPort>,
        event_publisher: Arc<dyn EventPublisherPort>,
    ) -> Result<Self, FullTextSearchError> {
        info!("Initializing Tantivy search integration");
        
        let index_reader = {
            let idx = index.index.read()
                .map_err(|e| FullTextSearchError::SearchIndexError(format!("Failed to acquire index read lock: {}", e)))?;
            idx
                .reader_builder()
                .reload_policy(ReloadPolicy::OnCommitWithDelay)
                .try_into()
                .map_err(|e| FullTextSearchError::SearchIndexError(format!("Failed to create index reader: {}", e)))?
        };
        
        Ok(Self {
            index,
            index_reader: Arc::new(index_reader),
            search_index,
            artifact_repository,
            event_publisher,
        })
    }
}

#[async_trait]
impl SearchIndexPort for TantivySearchIntegration {
    async fn search(
        &self,
        query: &SearchQuery,
    ) -> Result<SearchResults, FullTextSearchError> {
        debug!(query = %query.q, "Searching in Tantivy integration");
        
        let searcher = self.index_reader.searcher();
        
        // Create query parser
        let query_parser = QueryParser::for_index(
            &searcher.index(),
            vec![
                self.index.name_field,
                self.index.version_field,
                self.index.description_field,
                self.index.content_field,
            ],
        );
        
        // Parse the query
        let parsed_query = query_parser
            .parse_query(&query.q)
            .map_err(|e| FullTextSearchError::SearchIndexError(format!("Failed to parse query: {}", e)))?;
        
        // Execute search
        let page = query.page.unwrap_or(1);
        let page_size = query.page_size.unwrap_or(20);
        let offset = (page - 1) * page_size;
        
        let top_docs = searcher
            .search(
                &parsed_query,
                &(TopDocs::with_limit(page_size).and_offset(offset)),
            )
            .map_err(|e| FullTextSearchError::SearchIndexError(format!("Search execution failed: {}", e)))?;
        
        // Convert results
        let mut artifacts = Vec::new();
        for (_score, doc_address) in top_docs {
            let retrieved_doc = match searcher.doc(doc_address) {
                Ok(doc) => doc,
                Err(e) => {
                    error!("Failed to retrieve document: {}", e);
                    continue;
                }
            };
            
            if let Some(artifact) = self.index.from_document(&retrieved_doc) {
                artifacts.push(artifact);
            }
        }
        
        // For simplicity, we're not getting the total count here
        // In a real implementation, we would use a Count collector
        let total_count = artifacts.len();
        
        Ok(SearchResults::new(artifacts, total_count, page, page_size))
    }
    
    async fn index_artifact(
        &self,
        artifact: &ArtifactDocument,
    ) -> Result<(), FullTextSearchError> {
        debug!(artifact_id = %artifact.id, "Indexing artifact in Tantivy integration");
        self.index.index_artifact(artifact).await
    }
    
    async fn get_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<SearchResults, FullTextSearchError> {
        debug!(page = page, page_size = page_size, "Getting all artifacts in Tantivy integration");
        self.index.get_all_artifacts(page, page_size).await
    }
}