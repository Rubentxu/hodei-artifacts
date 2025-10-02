/// Domain layer for hodei-iam

pub mod entities;
pub mod actions;

// Re-export for convenience
pub use entities::{User, Group, ServiceAccount, Namespace};
pub use actions::{CreateUserAction, CreateGroupAction};
