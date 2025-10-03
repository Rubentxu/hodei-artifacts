use crate::shared::domain::{Group, User};
/// Application ports (interfaces) for hodei-iam
///
/// This module defines the traits (ports) that the application layer uses
/// to interact with infrastructure concerns like persistence.

use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn save(&self, user: &User) -> Result<(), anyhow::Error>;
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, anyhow::Error>;
    async fn find_all(&self) -> Result<Vec<User>, anyhow::Error>;
}

#[async_trait]
pub trait GroupRepository: Send + Sync {
    async fn save(&self, group: &Group) -> Result<(), anyhow::Error>;
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, anyhow::Error>;
    async fn find_all(&self) -> Result<Vec<Group>, anyhow::Error>;
}

