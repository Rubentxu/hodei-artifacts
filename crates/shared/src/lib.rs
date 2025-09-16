// crates/shared/src/lib.rs

pub mod enums;
pub mod events;
pub mod hrn;
pub mod lifecycle;
pub mod models;
pub mod security;
pub mod testing;
pub mod attributes;

// Ergonomic re-export so crates can `use shared::HodeiResource;`
pub use security::HodeiResource;