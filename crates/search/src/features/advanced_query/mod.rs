pub mod parser;
pub mod integration;
pub mod di;
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;

#[cfg(test)]
pub mod advanced_query_test;

// Expose only the public parts of the feature.
pub use di::AdvancedQueryDIContainer;
pub use dto::{AdvancedSearchQuery, AdvancedSearchResults, ParsedQueryInfo};
pub use error::AdvancedQueryError;