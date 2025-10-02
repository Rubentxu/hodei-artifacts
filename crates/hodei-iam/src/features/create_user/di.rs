/// Dependency Injection for create_user feature

use std::sync::Arc;
use crate::shared::application::ports::UserRepository;
use super::use_case::CreateUserUseCase;

pub fn make_use_case(repo: Arc<dyn UserRepository>) -> CreateUserUseCase {
    CreateUserUseCase::new(repo)
}

