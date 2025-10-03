use super::use_case::CreateUserUseCase;
use crate::shared::application::ports::UserRepository;
/// Dependency Injection for create_user feature

use std::sync::Arc;

pub fn make_use_case(repo: Arc<dyn UserRepository>) -> CreateUserUseCase {
    CreateUserUseCase::new(repo)
}

