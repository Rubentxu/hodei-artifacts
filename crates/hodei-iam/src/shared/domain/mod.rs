/// Domain layer for hodei-iam

pub mod entities;
pub mod actions;

pub use actions::{CreateGroupAction, CreateUserAction};
// Re-export for convenience
pub use entities::{Group, Namespace, ServiceAccount, User};
