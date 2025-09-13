//! Search document domain models
//!
//! This module contains the core document entities and value objects
//! for the search bounded context.

use super::{DocumentId, IndexId, RelevanceScore, ResultPosition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Search document representing indexed content
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchDocument {
    pub id: DocumentId,
    pub index_id: IndexId,
    pub title: String,
    pub content: String,
    pub summary: Option<String>,
    pub metadata: DocumentMetadata,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl SearchDocument {
    pub fn new(
        id: DocumentId,
        index_id: IndexId,
        title: String,
        content: String,
        summary: Option<String>,
        metadata: DocumentMetadata,
    ) -> Self {
        let now = chrono::Utc::now();
        Self {
            id,
            index_id,
            title,
            content,
            summary,
            metadata,
            created_at: now,
            updated_at: now,
        }
    }
    
    pub fn with_timestamps(
        id: DocumentId,
        index_id: IndexId,
        title: String,
        content: String,
        summary: Option<String>,
        metadata: DocumentMetadata,
        created_at: chrono::DateTime<chrono::Utc>,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self {
            id,
            index_id,
            title,
            content,
            summary,
            metadata,
            created_at,
            updated_at,
        }
    }
    
    /// Get the document type from metadata
    pub fn document_type(&self) -> &str {
        self.metadata.get("document_type").unwrap_or("unknown")
    }
    
    /// Get the document author from metadata
    pub fn author(&self) -> Option<&str> {
        self.metadata.get("author")
    }
    
    /// Get the document tags from metadata
    pub fn tags(&self) -> Vec<&str> {
        self.metadata.get("tags")
            .map(|tags| tags.split(',').map(|s| s.trim()).collect())
            .unwrap_or_default()
    }
    
    /// Check if document matches a specific tag
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags().contains(&tag)
    }
    
    /// Update document metadata
    pub fn update_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
        self.updated_at = chrono::Utc::now();
    }
    
    /// Calculate document relevance for a query
    pub fn calculate_relevance(&self, query_terms: &[String]) -> RelevanceScore {
        let mut score = 0.0;
        let title_lower = self.title.to_lowercase();
        let content_lower = self.content.to_lowercase();
        
        for term in query_terms {
            let term_lower = term.to_lowercase();
            
            // Title matches are worth more
            if title_lower.contains(&term_lower) {
                score += 0.4;
            }
            
            // Content matches
            if content_lower.contains(&term_lower) {
                score += 0.2;
            }
            
            // Summary matches
            if let Some(summary) = &self.summary {
                if summary.to_lowercase().contains(&term_lower) {
                    score += 0.3;
                }
            }
            
            // Tag matches
            if self.has_tag(&term_lower) {
                score += 0.1;
            }
        }
        
        RelevanceScore::new(score)
    }
}

/// Document metadata key-value pairs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentMetadata(HashMap<String, String>);

impl DocumentMetadata {
    pub fn new() -> Self {
        Self(HashMap::new())
    }
    
    pub fn with_capacity(capacity: usize) -> Self {
        Self(HashMap::with_capacity(capacity))
    }
    
    pub fn insert(&mut self, key: String, value: String) -> Option<String> {
        self.0.insert(key, value)
    }
    
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.as_str())
    }
    
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }
    
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.0.remove(key)
    }
    
    pub fn len(&self) -> usize {
        self.0.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
    
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(|k| k.as_str())
    }
    
    pub fn values(&self) -> impl Iterator<Item = &str> {
        self.0.values().map(|v| v.as_str())
    }
}

impl Default for DocumentMetadata {
    fn default() -> Self {
        Self::new()
    }
}

impl From<HashMap<String, String>> for DocumentMetadata {
    fn from(map: HashMap<String, String>) -> Self {
        Self(map)
    }
}

impl From<DocumentMetadata> for HashMap<String, String> {
    fn from(metadata: DocumentMetadata) -> Self {
        metadata.0
    }
}

/// Search result with document and relevance information
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub document: SearchDocument,
    pub relevance_score: RelevanceScore,
    pub position: ResultPosition,
    pub highlights: Vec<String>,
    pub snippet: Option<String>,
}

impl SearchResult {
    pub fn new(
        document: SearchDocument,
        relevance_score: RelevanceScore,
        position: ResultPosition,
        highlights: Vec<String>,
        snippet: Option<String>,
    ) -> Self {
        Self {
            document,
            relevance_score,
            position,
            highlights,
            snippet,
        }
    }
    
    /// Check if this result is highly relevant
    pub fn is_highly_relevant(&self) -> bool {
        self.relevance_score.is_high_relevance()
    }
    
    /// Check if this result is in top positions
    pub fn is_top_result(&self) -> bool {
        self.position.is_top_ten()
    }
    
    /// Get a formatted snippet
    pub fn formatted_snippet(&self) -> String {
        self.snippet.as_ref()
            .unwrap_or(&self.document.summary.clone().unwrap_or_else(|| {
                // Generate a simple snippet from content
                let content = &self.document.content;
                if content.len() > 200 {
                    format!("{}...", &content[..200])
                } else {
                    content.clone()
                }
            }))
            .clone()
    }
}

/// Document field for indexing and searching
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DocumentField {
    Title,
    Content,
    Summary,
    Author,
    Tags,
    Metadata(String),
}

impl DocumentField {
    pub fn as_str(&self) -> &str {
        match self {
            DocumentField::Title => "title",
            DocumentField::Content => "content",
            DocumentField::Summary => "summary",
            DocumentField::Author => "author",
            DocumentField::Tags => "tags",
            DocumentField::Metadata(field) => field,
        }
    }
}

impl From<&str> for DocumentField {
    fn from(s: &str) -> Self {
        match s {
            "title" => DocumentField::Title,
            "content" => DocumentField::Content,
            "summary" => DocumentField::Summary,
            "author" => DocumentField::Author,
            "tags" => DocumentField::Tags,
            field => DocumentField::Metadata(field.to_string()),
        }
    }
}

impl std::fmt::Display for DocumentField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}