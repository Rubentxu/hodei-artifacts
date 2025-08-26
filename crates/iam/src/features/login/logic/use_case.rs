use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::features::login::command::{LoginCommand, LoginResponse};
use bcrypt::verify;
use jsonwebtoken::{encode, Header, EncodingKey};
use shared::UserId;
use serde::{Serialize, Deserialize};
use chrono::Utc;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: UserId,
    exp: usize,
}

pub struct LoginUseCase<'a> {
    user_repository: &'a dyn UserRepository,
}

impl<'a> LoginUseCase<'a> {
    pub fn new(user_repository: &'a dyn UserRepository) -> Self {
        Self { user_repository }
    }

    pub async fn execute(&self, command: LoginCommand) -> Result<LoginResponse, IamError> {
        let user = self.user_repository
            .find_by_username(&command.username)
            .await?
            .ok_or(IamError::NotFound)?;

        if !verify(&command.password, &user.password_hash).map_err(|e| IamError::BcryptError(e.to_string()))? {
            return Err(IamError::Unauthorized);
        }

        // TODO: Move secret to configuration
        let secret = "my_super_secret_key";
        let claims = Claims {
            sub: user.id,
            exp: (Utc::now() + chrono::Duration::hours(24)).timestamp() as usize,
        };
        let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_ref()))
            .map_err(|e| IamError::JwtError(e.to_string()))?;

        Ok(LoginResponse { token })
    }
}
