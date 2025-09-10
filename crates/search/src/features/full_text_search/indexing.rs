use async_trait::async_trait;
use std::sync::{Arc, RwLock};
use tantivy::{
    collector::TopDocs,
    doc,
    query::QueryParser,
    schema::*,
    Document,
    Index, IndexWriter, ReloadPolicy,
};
use tracing::{debug, info, error};

use crate::features::full_text_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    error::FullTextSearchError,
    ports::SearchIndexPort,
};

/// Tantivy-based indexer for full-text search
pub struct TantivyIndexer {
    index: Arc<RwLock<Index>>,
    index_writer: Arc<RwLock<IndexWriter>>,
    schema: Schema,
    id_field: Field,
    name_field: Field,
    version_field: Field,
    package_type_field: Field,
    repository_field: Field,
    description_field: Field,
    content_field: Field,
}

impl TantivyIndexer {
    pub fn new() -> Result<Self, FullTextSearchError> {
        info!("Initializing Tantivy indexer");
        
        // Define the schema
        let mut schema_builder = Schema::builder();
        
        let id_field = schema_builder.add_text_field("id", STRING | STORED);
        let name_field = schema_builder.add_text_field("name", TEXT | STORED);
        let version_field = schema_builder.add_text_field("version", STRING | STORED);
        let package_type_field = schema_builder.add_text_field("package_type", STRING | STORED);
        let repository_field = schema_builder.add_text_field("repository", STRING | STORED);
        let description_field = schema_builder.add_text_field("description", TEXT | STORED);
        let content_field = schema_builder.add_text_field("content", TEXT | STORED);
        
        let schema = schema_builder.build();
        
        // Create in-memory index
        let index = Index::create_in_ram(schema.clone());
        
        // Create index writer
        let index_writer = index
            .writer(50_000_000) // 50MB buffer
            .map_err(|e| FullTextSearchError::SearchIndexError(format!("Failed to create index writer: {}", e)))?;
        
        Ok(Self {
            index: Arc::new(RwLock::new(index)),
            index_writer: Arc::new(RwLock::new(index_writer)),
            schema,
            id_field,
            name_field,
            version_field,
            package_type_field,
            repository_field,
            description_field,
            content_field,
        })
    }
    
    fn to_document(&self, artifact: &ArtifactDocument) -> Document {
        doc! {
            self.id_field => artifact.id.clone(),
            self.name_field => artifact.name.to_lowercase(), // Store as lowercase for case-insensitive search
            self.version_field => artifact.version.clone(),
            self.package_type_field => artifact.package_type.clone(),
            self.repository_field => artifact.repository.clone(),
            self.description_field => artifact.description.clone(),
            self.content_field => artifact.content.clone(),
        }
    }
    
    fn from_document(&self, doc: &Document) -> Option<ArtifactDocument> {
        let id = doc.get_first(self.id_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let name = doc.get_first(self.name_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let version = doc.get_first(self.version_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let package_type = doc.get_first(self.package_type_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let repository = doc.get_first(self.repository_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let description = doc.get_first(self.description_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let content = doc.get_first(self.content_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        Some(ArtifactDocument {
            id,
            name,
            version,
            package_type,
            repository,
            description,
            content,
            score: 1.0, // Placeholder score
        })
    }
}

#[async_trait]
impl SearchIndexPort for TantivyIndexer {
    async fn search(
        &self,
        query: &SearchQuery,
    ) -> Result<SearchResults, FullTextSearchError> {
        debug!(query = %query.q, "Searching in Tantivy indexer");
        
        let index_reader = {
            let index = self.index.read()
                .map_err(|e| FullTextSearchError::SearchIndexError(format!("Failed to acquire index read lock: {}", e)))?;
            index
                .reader_builder()
                .reload_policy(ReloadPolicy::OnCommitWithDelay)
                .try_into()
                .map_err(|e| FullTextSearchError::SearchIndexError(format!("Failed to create index reader: {}", e)))?
        };
        
        let searcher = index_reader.searcher();
        
        // Create query parser
        let query_parser = QueryParser::for_index(
            &searcher.index(),
            vec![self.name_field, self.version_field, self.description_field, self.content_field],
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
            
            if let Some(artifact) = self.from_document(&retrieved_doc) {
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
        debug!(artifact_id = %artifact.id, "Indexing artifact in Tantivy indexer");
        
        let doc = self.to_document(artifact);
        
        {
            let mut writer = self.index_writer.write()
                .map_err(|e| FullTextSearchError::SearchIndexError(format!("Failed to acquire index writer lock: {}", e)))?;
            writer
                .add_document(doc)
                .map_err(|e| FullTextSearchError::SearchIndexError(format!("Failed to add document to index: {}", e)))?;
            
            writer
                .commit()
                .map_err(|e| FullTextSearchError::SearchIndexError(format!("Failed to commit index changes: {}", e)))?;
        }
        
        info!(artifact_id = %artifact.id, "Artifact indexed successfully");
        Ok(())
    }
    
    async fn get_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<SearchResults, FullTextSearchError> {
        debug!(page = page, page_size = page_size, "Getting all artifacts in Tantivy indexer");
        
        // For an empty query, we'll return all artifacts
        // This is a simplified implementation - in reality, we'd want to use a MatchAllQuery
        // and implement proper pagination
        
        // Since we're using an in-memory index for this basic implementation,
        // we'll just return an empty result set
        // A real implementation would query all documents
        
        Ok(SearchResults::new(Vec::new(), 0, page, page_size))
    }
}