// crates/shared/src/lib.rs

pub mod enums;
pub mod events;
pub mod hrn;
pub mod lifecycle;
pub mod models;
pub mod security;
pub mod attributes;


// Ergonomic re-export so crates can `use shared::HodeiResource;`
pub use security::HodeiResource;

// Re-export HRN types for other crates
pub use hrn::{Hrn, OrganizationId, HodeiPolicyId, UserId, TeamId, RepositoryId, ArtifactId, DashboardId, ReportId, AlertId};
