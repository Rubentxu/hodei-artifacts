//! Features layer for Search bounded context
//!
//! Contains all vertical slices (features) for search functionality
//! Following VSA principles - each feature is a self-contained use case
//!
pub mod basic_search;
pub mod advanced_search;
pub mod index_management;

// Re-export feature handlers for easy access
pub use basic_search::*;
pub use advanced_search::*;
pub use index_management::*;
