use serde::Deserialize;
use shared::UserId;

#[derive(Debug, Deserialize)]
pub struct GetUserQuery {
    pub id: UserId,
}
