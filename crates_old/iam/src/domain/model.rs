use serde::{Serialize, Deserialize};
use shared::{UserId, IsoTimestamp};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrincipalId(pub UserId);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAccount {
    pub id: UserId,
    pub email: String,
    pub created_at: IsoTimestamp,
}

impl UserAccount {
    pub fn new(email: String) -> Self { Self { id: UserId::new(), email, created_at: IsoTimestamp::now() } }
}

/// Modelo básico User para referencias en infraestructura
/// Siguiendo VSA: modelo mínimo necesario para que compile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub external_id: String,
    pub user_type: String,
    pub attributes: std::collections::HashMap<String, String>,
    pub created_by: UserId,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub status: UserStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserStatus {
    Active,
    Inactive,
    Suspended,
    Deleted,
}

impl User {
    pub fn new(external_id: String, user_type: String, created_by: UserId) -> Self {
        Self {
            id: UserId::new(),
            external_id,
            user_type,
            attributes: std::collections::HashMap::new(),
            created_by,
            created_at: chrono::Utc::now(),
            status: UserStatus::Active,
        }
    }
}
