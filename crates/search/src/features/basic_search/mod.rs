pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
pub mod event_adapter;
mod infrastructure;
pub mod ports;
pub mod mock;
// Backward compatibility for existing tests expecting test_utils
pub mod test_utils { pub use super::mock::*; }
pub mod use_case;

#[cfg(test)]
pub mod test_adapter { pub use super::mock::*; }
#[cfg(test)]
mod basic_search_test;

// Expose only the public parts of the feature.
pub use di::BasicSearchDIContainer;
pub use dto::{SearchQuery, SearchResults, ArtifactDocument};
pub use error::BasicSearchError;