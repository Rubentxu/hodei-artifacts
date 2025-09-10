use std::sync::{Arc, RwLock};
use tantivy::{
    collector::TopDocs,
    query::QueryParser,
    Index, IndexWriter, ReloadPolicy,
};
use tracing::{debug, info, error};

use crate::features::basic_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    error::BasicSearchError,
};

use super::{
    tantivy_schema::SearchSchema,
    tantivy_document_mapper::TantivyDocumentMapper,
};

pub struct TantivySearchIndex {
    index: Arc<RwLock<Index>>,
    index_writer: Arc<RwLock<IndexWriter>>,
    schema: SearchSchema,
    document_mapper: TantivyDocumentMapper,
}

impl TantivySearchIndex {
    pub fn new() -> Result<Self, BasicSearchError> {
        info!("Initializing Tantivy search index");
        
        let schema = SearchSchema::new();
        
        // Create in-memory index
        let index = Index::create_in_ram(schema.schema.clone());
        
        // Create index writer
        let index_writer = index
            .writer(50_000_000) // 50MB buffer
            .map_err(|e| BasicSearchError::SearchIndexError(format!("Failed to create index writer: {}", e)))?;
        
        let document_mapper = TantivyDocumentMapper::new(
            schema.id_field(),
            schema.name_field(),
            schema.version_field(),
            schema.package_type_field(),
            schema.repository_field(),
        );
        
        Ok(Self {
            index: Arc::new(RwLock::new(index)),
            index_writer: Arc::new(RwLock::new(index_writer)),
            schema,
            document_mapper,
        })
    }
    
    pub async fn search(&self, query: &SearchQuery) -> Result<SearchResults, BasicSearchError> {
        debug!(query = %query.q, "Searching in Tantivy index");
        
        let index_reader = {
            let index = self.index.read()
                .map_err(|e| BasicSearchError::SearchIndexError(format!("Failed to acquire index read lock: {}", e)))?;
            index
                .reader_builder()
                .reload_policy(ReloadPolicy::OnCommitWithDelay)
                .try_into()
                .map_err(|e| BasicSearchError::SearchIndexError(format!("Failed to create index reader: {}", e)))?
        };
        
        let searcher = index_reader.searcher();
        
        // Create query parser
        let query_parser = QueryParser::for_index(
            searcher.index(),
            vec![self.schema.name_field(), self.schema.version_field()],
        );
        
        // Parse the query
        let parsed_query = query_parser
            .parse_query(&query.q)
            .map_err(|e| BasicSearchError::SearchIndexError(format!("Failed to parse query: {}", e)))?;
        
        // Execute search
        let page = query.page.unwrap_or(1);
        let page_size = query.page_size.unwrap_or(20);
        let offset = (page - 1) * page_size;
        
        let top_docs = searcher
            .search(
                &parsed_query,
                &(TopDocs::with_limit(page_size).and_offset(offset)),
            )
            .map_err(|e| BasicSearchError::SearchIndexError(format!("Search execution failed: {}", e)))?;
        
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
            
            if let Some(artifact) = self.document_mapper.from_document(&retrieved_doc) {
                artifacts.push(artifact);
            }
        }
        
        // For simplicity, we're not getting the total count here
        // In a real implementation, we would use a Count collector
        let total_count = artifacts.len();
        
        Ok(SearchResults::new(artifacts, total_count, page, page_size))
    }
    
    pub async fn index_artifact(&self, artifact: &ArtifactDocument) -> Result<(), BasicSearchError> {
        debug!(artifact_id = %artifact.id, "Indexing artifact in Tantivy");
        
        let doc = self.document_mapper.to_document(artifact);
        
        {
            let mut writer = self.index_writer.write()
                .map_err(|e| BasicSearchError::SearchIndexError(format!("Failed to acquire index writer lock: {}", e)))?;
            writer
                .add_document(doc)
                .map_err(|e| BasicSearchError::SearchIndexError(format!("Failed to add document to index: {}", e)))?;
            
            writer
                .commit()
                .map_err(|e| BasicSearchError::SearchIndexError(format!("Failed to commit index changes: {}", e)))?;
        }
        
        info!(artifact_id = %artifact.id, "Artifact indexed successfully");
        Ok(())
    }
    
    pub async fn get_all_artifacts(&self, page: usize, page_size: usize) -> Result<SearchResults, BasicSearchError> {
        debug!(page = page, page_size = page_size, "Getting all artifacts");
        
        // For an empty query, we'll return all artifacts
        // This is a simplified implementation - in reality, we'd want to use a MatchAllQuery
        // and implement proper pagination
        
        // Since we're using an in-memory index for this basic implementation,
        // we'll just return an empty result set
        // A real implementation would query all documents
        
        Ok(SearchResults::new(Vec::new(), 0, page, page_size))
    }
}