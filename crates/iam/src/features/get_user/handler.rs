use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::domain::user::User;
use crate::features::get_user::query::GetUserQuery;
use crate::features::get_user::logic::use_case::GetUserUseCase;

pub async fn handle_get_user(
    user_repository: &dyn UserRepository,
    query: GetUserQuery,
) -> Result<User, IamError> {
    let use_case = GetUserUseCase::new(user_repository);
    use_case.execute(query).await
}
