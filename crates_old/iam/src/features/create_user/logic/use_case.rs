use crate::application::ports::UserRepository;
use crate::domain::user::{User, UserStatus};
use crate::error::IamError;
use crate::features::create_user::command::CreateUserCommand;
use crate::features::create_user::logic::validate::validate_command;
use shared::UserId;

pub struct CreateUserUseCase<'a> {
    user_repository: &'a dyn UserRepository,
}

impl<'a> CreateUserUseCase<'a> {
    pub fn new(user_repository: &'a dyn UserRepository) -> Self {
        Self { user_repository }
    }

    pub async fn execute(&self, cmd: CreateUserCommand) -> Result<UserId, IamError> {
        validate_command(&cmd)?;

        if self.user_repository.find_by_username(&cmd.username).await?.is_some() {
            return Err(IamError::ValidationError("Username already exists".to_string()));
        }

        let password_hash = bcrypt::hash(&cmd.password, bcrypt::DEFAULT_COST)
            .map_err(|e| IamError::InternalError(e.to_string()))?;

        let new_user = User {
            id: UserId::new(),
            username: cmd.username,
            email: cmd.email,
            password_hash,
            status: UserStatus::Active,
            attributes: cmd.attributes,
            groups: vec![],
            policies: vec![],
        };

        self.user_repository.save(&new_user).await?;

        Ok(new_user.id)
    }
}
