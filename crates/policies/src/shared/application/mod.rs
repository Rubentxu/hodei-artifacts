// application layer - only schema-related functionality remains
// Engine module gated behind legacy_infra during refactor
#[cfg(feature = "legacy_infra")]
mod engine;

pub mod di_helpers;

#[cfg(feature = "legacy_infra")]
pub use engine::EngineBuilder;

// Stub EngineBuilder when legacy_infra is disabled
#[cfg(not(feature = "legacy_infra"))]
#[derive(Debug, Clone)]
pub struct EngineBuilder;

#[cfg(not(feature = "legacy_infra"))]
impl EngineBuilder {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(feature = "legacy_infra"))]
impl Default for EngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}
