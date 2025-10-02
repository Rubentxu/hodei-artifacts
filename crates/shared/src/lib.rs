// crates/shared/src/lib.rs

pub mod enums;
// pub mod events;  // Temporalmente desactivado - depende de Hrn
// pub mod lifecycle;  // Temporalmente desactivado - depende de Hrn
pub mod attributes;
pub mod models;
pub mod security;

// Ergonomic re-export so crates can `use shared::HodeiResource;`
pub use security::HodeiResource;
