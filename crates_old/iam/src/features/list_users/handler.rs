use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::domain::user::User;
use crate::features::list_users::query::ListUsersQuery;
use crate::features::list_users::logic::use_case::ListUsersUseCase;

pub async fn handle_list_users(
    user_repository: &dyn UserRepository,
    _query: ListUsersQuery,
) -> Result<Vec<User>, IamError> {
    let use_case = ListUsersUseCase::new(user_repository);
    use_case.execute().await
}
