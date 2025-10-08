//! Public API surface for the `hodei-iam` bounded context.

// Re-export public modules for external consumption.
// This allows consumers to `use hodei_iam::features::create_user;`
pub use crate::features::*;
pub use crate::infrastructure::*;
