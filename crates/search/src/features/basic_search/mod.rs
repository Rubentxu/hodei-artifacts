pub mod adapter;
pub mod api;
pub mod di;
pub mod dto;
pub mod error;
pub mod event_adapter;
pub mod infrastructure;
pub mod ports;
pub mod repository_adapter;
pub mod test_utils;
pub mod use_case;

#[cfg(test)]
pub mod test_adapter;
#[cfg(test)]
mod basic_search_test;

// Expose only the public parts of the feature.
pub use di::BasicSearchDIContainer;
pub use dto::{SearchQuery, SearchResults, ArtifactDocument};
pub use error::BasicSearchError;