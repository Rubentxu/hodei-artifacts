pub mod parser;
pub mod integration;
pub mod error;
pub mod dto;
pub mod advanced_query_test;

// Expose only the public parts of the feature.
pub use parser::{AdvancedQueryParser, ParsedQuery};
pub use integration::AdvancedQueryIntegration;
pub use error::AdvancedQueryError;
pub use dto::{AdvancedSearchQuery, AdvancedSearchResults};