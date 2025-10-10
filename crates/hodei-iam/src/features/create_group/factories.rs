//! Factory for creating the CreateGroup use case
//!
//! This module follows the Java Config pattern for dependency injection:
//! - Factories receive already constructed dependencies
//! - Factories return trait objects for the use case
//! - No complex generics, just trait objects for maximum flexibility

use std::sync::Arc;
use tracing::info;

use crate::features::create_group::ports::{CreateGroupPort, CreateGroupUseCasePort, HrnGenerator};
use crate::features::create_group::use_case::CreateGroupUseCase;

/// Create the CreateGroup use case with injected dependencies
///
/// This factory follows the Java Config pattern - it receives already
/// constructed dependencies and assembles the use case.
///
/// # Arguments
///
/// * `persister` - Repository for persisting groups
/// * `hrn_generator` - Generator for creating HRNs
///
/// # Returns
///
/// Arc<dyn CreateGroupUseCasePort> - The use case as a trait object
///
/// # Example
///
/// ```rust,ignore
/// let group_repo = Arc::new(SurrealGroupAdapter::new(db));
/// let hrn_generator = Arc::new(DefaultHrnGenerator::new());
///
/// let create_group = create_create_group_use_case(
///     group_repo,
///     hrn_generator,
/// );
/// ```
pub fn create_create_group_use_case(
    persister: Arc<dyn CreateGroupPort>,
    hrn_generator: Arc<dyn HrnGenerator>,
) -> Arc<dyn CreateGroupUseCasePort> {
    info!("Creating CreateGroup use case");
    Arc::new(CreateGroupUseCase::new(persister, hrn_generator))
}

/// Alternative factory that accepts owned dependencies
///
/// This is useful when you have dependencies that are not yet wrapped in Arc
/// and you want the factory to handle the Arc wrapping.
///
/// # Arguments
///
/// * `persister` - Repository for persisting groups
/// * `hrn_generator` - Generator for creating HRNs
///
/// # Returns
///
/// Arc<dyn CreateGroupUseCasePort> - The use case as a trait object
pub fn create_create_group_use_case_from_owned<P, G>(
    persister: P,
    hrn_generator: G,
) -> Arc<dyn CreateGroupUseCasePort>
where
    P: CreateGroupPort + 'static,
    G: HrnGenerator + 'static,
{
    info!("Creating CreateGroup use case from owned dependencies");
    Arc::new(CreateGroupUseCase::new(
        Arc::new(persister),
        Arc::new(hrn_generator),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::create_group::dto::CreateGroupCommand;
    use crate::features::create_group::mocks::{MockCreateGroupPort, MockHrnGenerator};

    #[tokio::test]
    async fn test_factory_creates_use_case() {
        let persister: Arc<dyn CreateGroupPort> = Arc::new(MockCreateGroupPort::new());
        let hrn_generator: Arc<dyn HrnGenerator> = Arc::new(MockHrnGenerator::new());

        let use_case = create_create_group_use_case(persister, hrn_generator);

        let command = CreateGroupCommand {
            group_name: "test-group".to_string(),
            tags: None,
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_factory_from_owned_works() {
        let persister = MockCreateGroupPort::new();
        let hrn_generator = MockHrnGenerator::new();

        let use_case = create_create_group_use_case_from_owned(persister, hrn_generator);

        let command = CreateGroupCommand {
            group_name: "test-group".to_string(),
            tags: None,
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }
}
