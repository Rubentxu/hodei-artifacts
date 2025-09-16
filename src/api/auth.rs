use axum::http::StatusCode;
use axum::request::Parts;
use axum::{extract::FromRequestParts, response::IntoResponse};

#[derive(Debug, Clone)]
pub struct UserIdentity {
    pub user_id: String,
    pub username: String,
}

impl<S> FromRequestParts<S> for UserIdentity
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request_parts(_parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(UserIdentity {
            user_id: "test-user-123".to_string(),
            username: "testuser".to_string(),
        })
    }
}
