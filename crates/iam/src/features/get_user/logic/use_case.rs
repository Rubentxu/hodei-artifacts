use crate::application::ports::UserRepository;
use crate::domain::user::User;
use crate::error::IamError;
use crate::features::get_user::query::GetUserQuery;

pub struct GetUserUseCase<'a> {
    user_repository: &'a dyn UserRepository,
}

impl<'a> GetUserUseCase<'a> {
    pub fn new(user_repository: &'a dyn UserRepository) -> Self {
        Self { user_repository }
    }

    pub async fn execute(&self, query: GetUserQuery) -> Result<User, IamError> {
        self.user_repository
            .find_by_id(&query.id)
            .await?
            .ok_or(IamError::NotFound)
    }
}
