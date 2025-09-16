//! Deprecated: use `features::search_full_text` instead.
//! This module is a thin compatibility shim that re-exports the consolidated
//! `search_full_text` feature to avoid breaking imports while we complete the
//! migration.

pub use crate::features::search_full_text::*;
// Note: We're not re-exporting anything from this feature since it's still in development
// and we want to maintain clean boundaries between features