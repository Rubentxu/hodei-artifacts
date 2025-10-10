//! Factory for creating the CreateUser use case
//!
//! This module follows the trait objects pattern for dependency injection:
//! - Factories receive Arc<dyn Trait> dependencies
//! - Factories return Arc<dyn UseCasePort> for maximum flexibility
//! - Easy testing with mock implementations

use std::sync::Arc;
use tracing::info;

use crate::features::create_user::ports::{CreateUserPort, CreateUserUseCasePort};
use crate::features::create_user::use_case::CreateUserUseCase;
use kernel::HrnGenerator;

/// Create the CreateUser use case with injected dependencies
///
/// This factory receives trait objects and returns a trait object,
/// making it simple to use from the Composition Root and easy to test.
///
/// # Arguments
///
/// * `persister` - Port for persisting users
/// * `hrn_generator` - Port for generating HRNs
///
/// # Returns
///
/// Arc<dyn CreateUserUseCasePort> - The use case as a trait object
///
/// # Example
///
/// ```rust,ignore
/// let user_repo = Arc::new(SurrealUserAdapter::new(db));
/// let hrn_generator = Arc::new(UuidHrnGenerator::new("hodei".to_string(), "iam".to_string(), "account".to_string()));
///
/// let create_user = create_user_use_case(
///     user_repo,
///     hrn_generator,
/// );
/// ```
pub fn create_user_use_case(
    persister: Arc<dyn CreateUserPort>,
    hrn_generator: Arc<dyn HrnGenerator>,
) -> Arc<dyn CreateUserUseCasePort> {
    info!("Creating CreateUser use case");
    Arc::new(CreateUserUseCase::new(persister, hrn_generator))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::create_user::dto::CreateUserCommand;
    use crate::features::create_user::mocks::{MockCreateUserPort, MockHrnGenerator};

    #[tokio::test]
    async fn test_factory_creates_use_case() {
        let persister: Arc<dyn CreateUserPort> = Arc::new(MockCreateUserPort::new());
        let hrn_generator: Arc<dyn HrnGenerator> = Arc::new(MockHrnGenerator::new());

        let use_case = create_user_use_case(persister, hrn_generator);

        let command = CreateUserCommand {
            name: "test-user".to_string(),
            email: "test@example.com".to_string(),
            tags: None,
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }
}
