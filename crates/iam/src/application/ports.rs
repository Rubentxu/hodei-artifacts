use async_trait::async_trait;
use shared::{UserId, ServiceAccountId};
use crate::domain::{User, ServiceAccount, Principal, Policy, PolicyId};
use crate::error::IamError;
use cedar_policy::{Request, Response, Decision};
use std::collections::HashSet;

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, IamError>;
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, IamError>;
    async fn save(&self, user: &User) -> Result<(), IamError>;
    async fn find_all(&self) -> Result<Vec<User>, IamError>;
    async fn delete(&self, id: &UserId) -> Result<(), IamError>;
}

#[async_trait]
pub trait ServiceAccountRepository: Send + Sync {
    async fn find_by_id(&self, id: &ServiceAccountId) -> Result<Option<ServiceAccount>, IamError>;
    async fn save(&self, sa: &ServiceAccount) -> Result<(), IamError>;
}

// Un trait unificado puede ser útil para la capa de autorización
#[async_trait]
pub trait PrincipalRepository: Send + Sync {
    async fn find_principal_by_id(&self, id: &str) -> Result<Option<Principal>, IamError>;
}

#[async_trait]
pub trait Authorization: Send + Sync {
    async fn is_authorized(&self, request: Request) -> Result<Response, IamError>;
}

#[async_trait]
pub trait PolicyRepository: Send + Sync {
    async fn find_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError>;
    async fn save(&self, policy: &Policy) -> Result<(), IamError>;
    async fn delete(&self, id: &PolicyId) -> Result<(), IamError>;
    async fn find_all(&self) -> Result<Vec<Policy>, IamError>;
}

pub trait PolicyValidator: Send + Sync {
    fn validate_policy_syntax(&self, policy_content: &str) -> Result<(), IamError>;
    fn validate_policy_semantics(&self, policy_content: &str, entities: HashSet<String>) -> Result<(), IamError>;
}

#[async_trait]
pub trait DecisionCache: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<(Decision, HashSet<PolicyId>)>, IamError>;
    async fn set(&self, key: &str, decision: Decision, reason: HashSet<PolicyId>, ttl_seconds: usize) -> Result<(), IamError>;
}