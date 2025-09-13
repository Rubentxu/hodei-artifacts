//! Search index domain models
//!
//! This module contains the index-related domain entities and value objects
//! for managing search indices and their configuration.

use super::IndexId;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Search index configuration and metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchIndex {
    pub id: IndexId,
    pub name: String,
    pub description: Option<String>,
    pub config: IndexConfig,
    pub statistics: IndexStatistics,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub status: IndexStatus,
}

impl SearchIndex {
    pub fn new(
        id: IndexId,
        name: String,
        description: Option<String>,
        config: IndexConfig,
    ) -> Self {
        let now = chrono::Utc::now();
        Self {
            id,
            name,
            description,
            config,
            statistics: IndexStatistics::new(),
            created_at: now,
            updated_at: now,
            status: IndexStatus::Creating,
        }
    }
    
    pub fn with_timestamps(
        id: IndexId,
        name: String,
        description: Option<String>,
        config: IndexConfig,
        created_at: chrono::DateTime<chrono::Utc>,
        updated_at: chrono::DateTime<chrono::Utc>,
        status: IndexStatus,
        statistics: IndexStatistics,
    ) -> Self {
        Self {
            id,
            name,
            description,
            config,
            statistics,
            created_at,
            updated_at,
            status,
        }
    }
    
    /// Check if the index is ready for searching
    pub fn is_ready(&self) -> bool {
        matches!(self.status, IndexStatus::Active | IndexStatus::Optimizing)
    }
    
    /// Check if the index is in a healthy state
    pub fn is_healthy(&self) -> bool {
        !matches!(self.status, IndexStatus::Error(_) | IndexStatus::Corrupted)
    }
    
    /// Mark index as active
    pub fn mark_active(&mut self) {
        self.status = IndexStatus::Active;
        self.updated_at = chrono::Utc::now();
    }
    
    /// Mark index as optimizing
    pub fn mark_optimizing(&mut self) {
        self.status = IndexStatus::Optimizing;
        self.updated_at = chrono::Utc::now();
    }
    
    /// Mark index with error
    pub fn mark_error(&mut self, error: String) {
        self.status = IndexStatus::Error(error);
        self.updated_at = chrono::Utc::now();
    }
    
    /// Update statistics
    pub fn update_statistics(&mut self, update: impl FnOnce(&mut IndexStatistics)) {
        update(&mut self.statistics);
        self.updated_at = chrono::Utc::now();
    }
    
    /// Add document to statistics
    pub fn add_document(&mut self, doc_size_bytes: u64) {
        self.statistics.total_documents += 1;
        self.statistics.total_size_bytes += doc_size_bytes;
        self.updated_at = chrono::Utc::now();
    }
    
    /// Remove document from statistics
    pub fn remove_document(&mut self, doc_size_bytes: u64) {
        self.statistics.total_documents = self.statistics.total_documents.saturating_sub(1);
        self.statistics.total_size_bytes = self.statistics.total_size_bytes.saturating_sub(doc_size_bytes);
        self.updated_at = chrono::Utc::now();
    }
}

/// Index configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexConfig {
    pub max_results: usize,
    pub snippet_length: usize,
    pub highlight_enabled: bool,
    pub fuzzy_search_enabled: bool,
    pub synonym_search_enabled: bool,
    pub auto_optimize_enabled: bool,
    pub optimization_threshold: u32,
    pub language: String,
    pub analyzer: AnalyzerConfig,
    pub ranking: RankingConfig,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            max_results: 100,
            snippet_length: 200,
            highlight_enabled: true,
            fuzzy_search_enabled: true,
            synonym_search_enabled: false,
            auto_optimize_enabled: true,
            optimization_threshold: 1000,
            language: "en".to_string(),
            analyzer: AnalyzerConfig::default(),
            ranking: RankingConfig::default(),
        }
    }
}

/// Text analyzer configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalyzerConfig {
    pub stemming_enabled: bool,
    pub stop_words_enabled: bool,
    pub custom_stop_words: Vec<String>,
    pub min_token_length: usize,
    pub max_token_length: usize,
    pub tokenizer: TokenizerType,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            stemming_enabled: true,
            stop_words_enabled: true,
            custom_stop_words: Vec::new(),
            min_token_length: 2,
            max_token_length: 100,
            tokenizer: TokenizerType::Standard,
        }
    }
}

/// Tokenizer type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenizerType {
    Standard,
    Whitespace,
    Keyword,
    Custom(String),
}

/// Ranking configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankingConfig {
    pub title_boost: f64,
    pub content_boost: f64,
    pub summary_boost: f64,
    pub freshness_boost: f64,
    pub popularity_boost: f64,
    pub tag_boost: f64,
    pub author_boost: f64,
}

impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            title_boost: 2.0,
            content_boost: 1.0,
            summary_boost: 1.5,
            freshness_boost: 0.5,
            popularity_boost: 0.3,
            tag_boost: 0.8,
            author_boost: 0.2,
        }
    }
}

/// Index statistics
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexStatistics {
    pub total_documents: u64,
    pub total_size_bytes: u64,
    pub total_terms: u64,
    pub average_document_size_bytes: f64,
    pub last_optimization: Option<chrono::DateTime<chrono::Utc>>,
    pub creation_time: chrono::DateTime<chrono::Utc>,
}

impl IndexStatistics {
    pub fn new() -> Self {
        Self {
            total_documents: 0,
            total_size_bytes: 0,
            total_terms: 0,
            average_document_size_bytes: 0.0,
            last_optimization: None,
            creation_time: chrono::Utc::now(),
        }
    }
    
    pub fn with_documents(documents: u64, total_size: u64) -> Self {
        Self {
            total_documents: documents,
            total_size_bytes: total_size,
            total_terms: 0,
            average_document_size_bytes: if documents > 0 {
                total_size as f64 / documents as f64
            } else {
                0.0
            },
            last_optimization: None,
            creation_time: chrono::Utc::now(),
        }
    }
    
    /// Calculate average document size
    pub fn update_average_size(&mut self) {
        self.average_document_size_bytes = if self.total_documents > 0 {
            self.total_size_bytes as f64 / self.total_documents as f64
        } else {
            0.0
        };
    }
    
    /// Record optimization
    pub fn record_optimization(&mut self) {
        self.last_optimization = Some(chrono::Utc::now());
    }
    
    /// Check if index needs optimization
    pub fn needs_optimization(&self, threshold: u32) -> bool {
        match self.last_optimization {
            Some(last_opt) => {
                let duration = chrono::Utc::now().signed_duration_since(last_opt);
                duration.num_minutes() > threshold as i64
            }
            None => true, // Never optimized
        }
    }
}

/// Index status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IndexStatus {
    Creating,
    Active,
    Optimizing,
    Idle,
    Error(String),
    Corrupted,
    Maintenance,
}

impl IndexStatus {
    pub fn as_str(&self) -> &str {
        match self {
            IndexStatus::Creating => "creating",
            IndexStatus::Active => "active",
            IndexStatus::Optimizing => "optimizing",
            IndexStatus::Idle => "idle",
            IndexStatus::Error(_) => "error",
            IndexStatus::Corrupted => "corrupted",
            IndexStatus::Maintenance => "maintenance",
        }
    }
    
    pub fn is_operational(&self) -> bool {
        matches!(self, IndexStatus::Active | IndexStatus::Idle | IndexStatus::Optimizing)
    }
}

/// Index field definition
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexField {
    pub name: String,
    pub field_type: FieldType,
    pub indexed: bool,
    pub stored: bool,
    pub analyzed: bool,
    pub required: bool,
    pub boost: Option<f64>,
}

impl IndexField {
    pub fn new(name: String, field_type: FieldType) -> Self {
        Self {
            name,
            field_type,
            indexed: true,
            stored: true,
            analyzed: true,
            required: false,
            boost: None,
        }
    }
    
    pub fn text(name: String) -> Self {
        Self::new(name, FieldType::Text)
    }
    
    pub fn keyword(name: String) -> Self {
        Self {
            name,
            field_type: FieldType::Keyword,
            indexed: true,
            stored: true,
            analyzed: false,
            required: false,
            boost: None,
        }
    }
    
    pub fn numeric(name: String) -> Self {
        Self {
            name,
            field_type: FieldType::Numeric,
            indexed: true,
            stored: true,
            analyzed: false,
            required: false,
            boost: None,
        }
    }
    
    pub fn date(name: String) -> Self {
        Self {
            name,
            field_type: FieldType::Date,
            indexed: true,
            stored: true,
            analyzed: false,
            required: false,
            boost: None,
        }
    }
    
    pub fn boolean(name: String) -> Self {
        Self {
            name,
            field_type: FieldType::Boolean,
            indexed: true,
            stored: true,
            analyzed: false,
            required: false,
            boost: None,
        }
    }
}

/// Field type for indexing
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldType {
    Text,
    Keyword,
    Numeric,
    Date,
    Boolean,
    Json,
}

/// Index schema definition
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexSchema {
    pub fields: Vec<IndexField>,
    pub unique_fields: HashSet<String>,
    pub default_search_field: Option<String>,
}

impl IndexSchema {
    pub fn new() -> Self {
        Self {
            fields: Vec::new(),
            unique_fields: HashSet::new(),
            default_search_field: None,
        }
    }
    
    pub fn add_field(&mut self, field: IndexField) {
        self.fields.push(field);
    }
    
    pub fn add_unique_field(&mut self, field_name: String) {
        self.unique_fields.insert(field_name);
    }
    
    pub fn set_default_search_field(&mut self, field_name: String) {
        self.default_search_field = Some(field_name);
    }
    
    pub fn get_field(&self, name: &str) -> Option<&IndexField> {
        self.fields.iter().find(|f| f.name == name)
    }
    
    pub fn has_field(&self, name: &str) -> bool {
        self.fields.iter().any(|f| f.name == name)
    }
    
    pub fn is_unique_field(&self, name: &str) -> bool {
        self.unique_fields.contains(name)
    }
}