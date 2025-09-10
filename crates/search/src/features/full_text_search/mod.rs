//! Full-text search feature for Hodei Artifacts
//!
//! This module implements full-text search capabilities across artifact content
//! and metadata, providing advanced search functionality with relevance ranking
//! using the BM25 algorithm.

pub mod adapter;
pub mod api;
pub mod di;
pub mod dto;
pub mod error;
pub mod event_adapter;
pub mod indexing;
pub mod integration;
pub mod ports;
pub mod repository_adapter;
pub mod scoring;
pub mod test_utils;
pub mod use_case;

#[cfg(test)]
pub mod full_text_search_test;

// Expose only the public parts of the feature.
// Note: We're not re-exporting anything from this feature since it's still in development
// and we want to maintain clean boundaries between features