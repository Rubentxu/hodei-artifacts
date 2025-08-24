// Crate analytics - placeholder inicial
pub mod domain;
pub mod application;
pub mod features;
pub mod infrastructure;
pub mod error;

// Re-export convenientes dentro del bounded context
pub use error::AnalyticsError;
pub use domain::model::*;
pub use domain::event::*;
pub use application::ports::*;
