//! Full-text search feature for Hodei Artifacts
//!
//! This module implements full-text search capabilities across artifact content
//! and metadata, providing advanced search functionality with relevance ranking
//! using the BM25 algorithm.

pub mod indexing;
pub mod integration;
pub mod scoring;
pub mod error;
pub mod dto;
pub mod full_text_search_test;

// Expose only the public parts of the feature.
pub use indexing::FullTextIndexer;
pub use integration::FullTextSearchIntegration;
pub use scoring::BM25Scorer;
pub use error::FullTextSearchError;
pub use dto::{FullTextSearchQuery, FullTextSearchResults, IndexedArtifact};