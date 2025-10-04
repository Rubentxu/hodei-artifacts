/// Application ports (interfaces) for hodei-iam
///
/// This module defines the traits (ports) that the application layer uses
/// to interact with infrastructure concerns like persistence.
use crate::shared::domain::{Group, User};
use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;

// Export authorization ports
pub mod authorization;
pub use authorization::{IamPolicyProvider, IamPolicyProviderError};

// Export error types
pub mod errors;
pub use errors::{GroupRepositoryError, PolicyRepositoryError, UserRepositoryError};

/// Repository trait for User entity operations
#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Save (create or update) a user
    async fn save(&self, user: &User) -> Result<(), UserRepositoryError>;

    /// Find a user by its HRN
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, UserRepositoryError>;

    /// Find all users
    async fn find_all(&self) -> Result<Vec<User>, UserRepositoryError>;
}

/// Repository trait for Group entity operations
#[async_trait]
pub trait GroupRepository: Send + Sync {
    /// Save (create or update) a group
    async fn save(&self, group: &Group) -> Result<(), GroupRepositoryError>;

    /// Find a group by its HRN
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, GroupRepositoryError>;

    /// Find all groups
    async fn find_all(&self) -> Result<Vec<Group>, GroupRepositoryError>;
}
