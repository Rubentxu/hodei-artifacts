use serde::{Serialize, Deserialize};
use shared::{RepositoryId, UserId, IsoTimestamp};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryName(pub String);
impl RepositoryName { pub fn new(v: impl Into<String>) -> Self { Self(v.into()) } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryDescription(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub id: RepositoryId,
    pub name: RepositoryName,
    pub description: Option<RepositoryDescription>,
    pub created_at: IsoTimestamp,
    pub created_by: UserId,
}

impl Repository {
    pub fn new(id: RepositoryId, name: RepositoryName, description: Option<RepositoryDescription>, created_by: UserId) -> Self {
        Self { id, name, description, created_at: IsoTimestamp::now(), created_by }
    }
}

