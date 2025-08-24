//! Modelos base compartidos estables (Value Objects genéricos, IDs, etc.)
use uuid::Uuid;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactId(pub Uuid);

impl ArtifactId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepositoryId(pub Uuid);

impl RepositoryId { pub fn new() -> Self { Self(Uuid::new_v4()) } }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Uuid);
impl UserId { pub fn new() -> Self { Self(Uuid::new_v4()) } }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsoTimestamp(pub DateTime<Utc>);
impl IsoTimestamp { pub fn now() -> Self { Self(Utc::now()) } }

