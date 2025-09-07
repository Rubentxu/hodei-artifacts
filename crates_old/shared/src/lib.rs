pub mod domain;
pub mod application;

pub mod error;

// Re-export tipos comunes
pub use domain::model::*;
pub use domain::event::*;
pub use application::ports::*;
pub use error::SharedError;

