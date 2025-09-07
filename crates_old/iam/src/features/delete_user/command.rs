use serde::Deserialize;
use shared::UserId;

#[derive(Debug, Deserialize)]
pub struct DeleteUserCommand {
    pub id: UserId,
}
