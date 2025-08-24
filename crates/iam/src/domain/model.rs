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

