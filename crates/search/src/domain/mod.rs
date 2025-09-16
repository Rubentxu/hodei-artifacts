//! Search domain models and value objects
//!
//! This module contains the core domain entities for the search bounded context,
//! including search documents, indices, and search-specific domain logic.

pub mod document;
pub mod index;
pub mod query;
pub mod dashboard;
pub mod report;
pub mod alert;

// Re-export commonly used domain types
pub use document::*;
pub use index::*;
pub use query::*;
pub use dashboard::*;
pub use report::*;
pub use alert::*;

/// Search document identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DocumentId(String);

impl DocumentId {
    pub fn new(id: String) -> Self {
        Self(id)
    }
    
    pub fn as_str(&self) -> &str {
        &self.0
    }
    
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for DocumentId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for DocumentId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl std::fmt::Display for DocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Search index identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct IndexId(String);

impl IndexId {
    pub fn new(id: String) -> Self {
        Self(id)
    }
    
    pub fn as_str(&self) -> &str {
        &self.0
    }
    
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for IndexId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for IndexId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl std::fmt::Display for IndexId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Search relevance score
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct RelevanceScore(f64);

impl RelevanceScore {
    pub fn new(score: f64) -> Self {
        Self(score.max(0.0).min(1.0))
    }
    
    pub fn as_f64(&self) -> f64 {
        self.0
    }
    
    pub fn is_high_relevance(&self) -> bool {
        self.0 > 0.8
    }
    
    pub fn is_medium_relevance(&self) -> bool {
        self.0 > 0.5 && self.0 <= 0.8
    }
    
    pub fn is_low_relevance(&self) -> bool {
        self.0 <= 0.5
    }
}

impl Default for RelevanceScore {
    fn default() -> Self {
        Self(0.0)
    }
}

impl From<f64> for RelevanceScore {
    fn from(score: f64) -> Self {
        Self::new(score)
    }
}

/// Search result position
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResultPosition(usize);

impl ResultPosition {
    pub fn new(position: usize) -> Self {
        Self(position)
    }
    
    pub fn as_usize(&self) -> usize {
        self.0
    }
    
    pub fn is_first(&self) -> bool {
        self.0 == 0
    }
    
    pub fn is_top_ten(&self) -> bool {
        self.0 < 10
    }
}

impl From<usize> for ResultPosition {
    fn from(position: usize) -> Self {
        Self(position)
    }
}

impl std::fmt::Display for ResultPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0 + 1) // Display as 1-based
    }
}