use super::ports::CreateGroupPort;
use super::use_case::CreateGroupUseCase;
use crate::infrastructure::hrn_generator::HrnGenerator;
use std::sync::Arc;

/// Factory for creating CreateGroupUseCase instances
///
/// This factory encapsulates the dependency injection logic for the
/// CreateGroupUseCase, making it easier to construct instances with
/// different implementations of the ports.
pub struct CreateGroupUseCaseFactory;

impl CreateGroupUseCaseFactory {
    /// Build a CreateGroupUseCase instance
    ///
    /// # Arguments
    /// * `persister` - Implementation of CreateGroupPort for persistence
    /// * `hrn_generator` - Implementation of HrnGenerator for HRN generation
    ///
    /// # Returns
    /// * A new CreateGroupUseCase instance
    pub fn build<P, G>(persister: Arc<P>, hrn_generator: Arc<G>) -> CreateGroupUseCase<P, G>
    where
        P: CreateGroupPort,
        G: HrnGenerator,
    {
        CreateGroupUseCase::new(persister, hrn_generator)
    }
}
