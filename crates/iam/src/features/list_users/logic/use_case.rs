use crate::application::ports::UserRepository;
use crate::domain::user::User;
use crate::error::IamError;

pub struct ListUsersUseCase<'a> {
    user_repository: &'a dyn UserRepository,
}

impl<'a> ListUsersUseCase<'a> {
    pub fn new(user_repository: &'a dyn UserRepository) -> Self {
        Self { user_repository }
    }

    pub async fn execute(&self) -> Result<Vec<User>, IamError> {
        self.user_repository.find_all().await
    }
}
