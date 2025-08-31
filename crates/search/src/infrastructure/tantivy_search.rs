use async_trait::async_trait;
use crate::application::ports::{AdvancedSearchIndex, SearchIndex};
use crate::domain::model::ArtifactSearchDocument;
use crate::error::{SearchError, SearchResult};
use crate::features::advanced_search::{AdvancedSearchQuery, AdvancedSearchResult};
use shared::{ArtifactId, RepositoryId, IsoTimestamp};
use std::path::PathBuf;
use std::sync::Arc;
use tantivy::{
    collector::TopDocs,
    directory::MmapDirectory,
    query::{BooleanQuery, Occur, Query, QueryParser, TermQuery},
    schema::{Field, Schema, STORED, TEXT, STRING, FAST, Value},
    Index, IndexReader, IndexWriter, Term, TantivyDocument,
};
use tokio::sync::RwLock;
use tracing::{info, warn};

pub struct TantivySearchIndex {
    index: Index,
    index_reader: IndexReader,
    index_writer: Arc<RwLock<IndexWriter>>,
    schema: Schema,
    fields: TantivyFields,
}

#[derive(Clone)]
struct TantivyFields {
    artifact_id: Field,
    repository_id: Field,
    name: Field,
    version: Field,
    description: Field,
    tags: Field,
    indexed_at: Field,
}

impl TantivySearchIndex {
    pub fn new(index_path: Option<PathBuf>) -> SearchResult<Self> {
        info!("Initializing Tantivy search index");

        // Build schema
        let mut schema_builder = Schema::builder();
        
        let fields = TantivyFields {
            artifact_id: schema_builder.add_text_field("artifact_id", STRING | STORED),
            repository_id: schema_builder.add_text_field("repository_id", STRING | STORED),
            name: schema_builder.add_text_field("name", TEXT | STORED),
            version: schema_builder.add_text_field("version", STRING | STORED),
            description: schema_builder.add_text_field("description", TEXT | STORED),
            tags: schema_builder.add_text_field("tags", TEXT | STORED),
            indexed_at: schema_builder.add_text_field("indexed_at", STRING | STORED | FAST),
        };
        
        let schema = schema_builder.build();

        // Create or open index
        let index = if let Some(path) = index_path {
            std::fs::create_dir_all(&path).map_err(|e| {
                SearchError::index_operation_failed("create index directory", format!("Failed to create index directory: {}", e))
            })?;
            
            let directory = MmapDirectory::open(&path).map_err(|e| {
                SearchError::index_operation_failed("open index directory", format!("Failed to open index directory: {}", e))
            })?;
            
            Index::open_or_create(directory, schema.clone()).map_err(|e| {
                SearchError::index_operation_failed("open or create index", format!("Failed to open or create index: {}", e))
            })?
        } else {
            // In-memory index for testing
            Index::create_in_ram(schema.clone())
        };

        // Create reader and writer
        let index_reader = index.reader().map_err(|e| {
            SearchError::index_operation_failed("create index reader", format!("Failed to create index reader: {}", e))
        })?;

        let index_writer = index.writer(50_000_000).map_err(|e| {
            SearchError::index_operation_failed("create index writer", format!("Failed to create index writer: {}", e))
        })?;

        info!("Tantivy search index initialized successfully");

        Ok(Self {
            index,
            index_reader,
            index_writer: Arc::new(RwLock::new(index_writer)),
            schema,
            fields,
        })
    }

    fn document_to_tantivy(&self, doc: &ArtifactSearchDocument) -> TantivyDocument {
        let mut tantivy_doc = TantivyDocument::default();
        
        tantivy_doc.add_text(self.fields.artifact_id, &doc.artifact_id.to_string());
        tantivy_doc.add_text(self.fields.repository_id, &doc.repository_id.to_string());
        tantivy_doc.add_text(self.fields.name, &doc.name);
        tantivy_doc.add_text(self.fields.version, &doc.version);
        
        if let Some(description) = &doc.description {
            tantivy_doc.add_text(self.fields.description, description);
        }
        
        for tag in &doc.tags {
            tantivy_doc.add_text(self.fields.tags, tag);
        }
        
        tantivy_doc.add_text(self.fields.indexed_at, &doc.indexed_at.to_string());
        
        tantivy_doc
    }

    fn tantivy_to_document(&self, tantivy_doc: &TantivyDocument, score: f32) -> SearchResult<ArtifactSearchDocument> {
        let artifact_id = tantivy_doc
            .get_first(self.fields.artifact_id)
            .and_then(|v| v.as_str())
            .ok_or_else(|| SearchError::index_operation_failed("retrieve document", "Missing artifact_id field"))?;

        let repository_id = tantivy_doc
            .get_first(self.fields.repository_id)
            .and_then(|v| v.as_str())
            .ok_or_else(|| SearchError::index_operation_failed("retrieve document", "Missing repository_id field"))?;

        let name = tantivy_doc
            .get_first(self.fields.name)
            .and_then(|v| v.as_str())
            .ok_or_else(|| SearchError::index_operation_failed("retrieve document", "Missing name field"))?;

        let version = tantivy_doc
            .get_first(self.fields.version)
            .and_then(|v| v.as_str())
            .ok_or_else(|| SearchError::index_operation_failed("retrieve document", "Missing version field"))?;

        let description = tantivy_doc
            .get_first(self.fields.description)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let tags: Vec<String> = tantivy_doc
            .get_all(self.fields.tags)
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect();

        let indexed_at_str = tantivy_doc
            .get_first(self.fields.indexed_at)
            .and_then(|v| v.as_str())
            .ok_or_else(|| SearchError::index_operation_failed("retrieve document", "Missing indexed_at field"))?;

        let indexed_at = indexed_at_str.parse::<IsoTimestamp>()
            .map_err(|e| SearchError::index_operation_failed("parse timestamp", format!("Invalid timestamp: {}", e)))?;

        Ok(ArtifactSearchDocument {
            artifact_id: artifact_id.parse::<ArtifactId>()
                .map_err(|e| SearchError::index_operation_failed("parse artifact_id", format!("Invalid artifact_id: {}", e)))?,
            repository_id: repository_id.parse::<RepositoryId>()
                .map_err(|e| SearchError::index_operation_failed("parse repository_id", format!("Invalid repository_id: {}", e)))?,
            name: name.to_string(),
            version: version.to_string(),
            description,
            tags,
            indexed_at,
            relevance_score: score as f64,
        })
    }
}

#[async_trait]
impl SearchIndex for TantivySearchIndex {
    async fn index(&self, document: &ArtifactSearchDocument) -> SearchResult<()> {
        let tantivy_doc = self.document_to_tantivy(document);
        
        let mut writer = self.index_writer.write().await;
        writer.add_document(tantivy_doc).map_err(|e| {
            SearchError::index_operation_failed("add document", format!("Failed to add document to index: {}", e))
        })?;
        
        writer.commit().map_err(|e| {
            SearchError::index_operation_failed("commit document", format!("Failed to commit document to index: {}", e))
        })?;

        info!("Document indexed successfully: {}", document.artifact_id);
        Ok(())
    }

    async fn search(&self, query: &str, repository_filter: Option<String>) -> SearchResult<Vec<ArtifactSearchDocument>> {
        // Reload the reader to see latest changes
        self.index_reader.reload()?;
        let searcher = self.index_reader.searcher();
        
        // Build query parser for name, description, and tags fields
        let query_parser = QueryParser::for_index(&self.index, vec![self.fields.name, self.fields.description, self.fields.tags]);
        
        let parsed_query = query_parser.parse_query(query).map_err(|e| {
            SearchError::invalid_query(format!("Failed to parse query '{}': {}", query, e))
        })?;

        // Add repository filter if provided
        let final_query: Box<dyn Query> = if let Some(repo_filter) = repository_filter {
            let repo_term = Term::from_field_text(self.fields.repository_id, &repo_filter);
            let repo_query = TermQuery::new(repo_term, tantivy::schema::IndexRecordOption::Basic);
            
            Box::new(BooleanQuery::new(vec![
                (Occur::Must, parsed_query),
                (Occur::Must, Box::new(repo_query)),
            ]))
        } else {
            parsed_query
        };

        // Execute search
        let top_docs = searcher.search(&final_query, &TopDocs::with_limit(100)).map_err(|e| {
            SearchError::query_failed(format!("Search execution failed: {}", e))
        })?;

        // Convert results
        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            let tantivy_doc = searcher.doc(doc_address).map_err(|e| {
                SearchError::index_operation_failed("retrieve document", format!("Failed to retrieve document: {}", e))
            })?;
            
            match self.tantivy_to_document(&tantivy_doc, score) {
                Ok(artifact_doc) => results.push(artifact_doc),
                Err(e) => {
                    warn!("Failed to convert document from index: {}", e);
                    continue;
                }
            }
        }

        info!("Search completed: query='{}', results={}", query, results.len());
        Ok(results)
    }
}

#[async_trait]
impl AdvancedSearchIndex for TantivySearchIndex {
    async fn advanced_search(&self, query: &AdvancedSearchQuery) -> SearchResult<AdvancedSearchResult> {
        // For now, implement basic search functionality
        // TODO: Implement advanced features like facets, complex filters, etc.
        // Reload the reader to see latest changes
        self.index_reader.reload()?;
        let searcher = self.index_reader.searcher();
        
        let query_parser = QueryParser::for_index(&self.index, vec![
            self.fields.name, 
            self.fields.description,
            self.fields.tags
        ]);
        
        let parsed_query = query_parser.parse_query(&query.q).map_err(|e| {
            SearchError::invalid_query(format!("Failed to parse advanced query '{}': {}", query.q, e))
        })?;

        let top_docs = searcher.search(&parsed_query, &TopDocs::with_limit(100)).map_err(|e| {
            SearchError::query_failed(format!("Advanced search execution failed: {}", e))
        })?;

        let hits: Vec<String> = top_docs.iter()
            .filter_map(|(_, doc_address)| {
                let doc_result = searcher.doc(*doc_address).ok();
                doc_result.and_then(|doc: TantivyDocument| {
                    doc.get_first(self.fields.name)
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
            })
            .collect();

        Ok(AdvancedSearchResult {
            total: hits.len() as u64,
            hits,
        })
    }
}