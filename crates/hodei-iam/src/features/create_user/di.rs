use super::ports::CreateUserPort;
use super::use_case::CreateUserUseCase;
use crate::infrastructure::hrn_generator::HrnGenerator;
use std::sync::Arc;

/// Factory for creating CreateUserUseCase instances
///
/// This factory encapsulates the dependency injection logic for the
/// CreateUserUseCase, making it easier to construct instances with
/// different implementations of the ports.
pub struct CreateUserUseCaseFactory;

impl CreateUserUseCaseFactory {
    /// Build a CreateUserUseCase instance
    ///
    /// # Arguments
    /// * `persister` - Implementation of CreateUserPort for persistence
    /// * `hrn_generator` - Implementation of HrnGenerator for HRN generation
    ///
    /// # Returns
    /// * A new CreateUserUseCase instance
    pub fn build<P, G>(persister: Arc<P>, hrn_generator: Arc<G>) -> CreateUserUseCase<P, G>
    where
        P: CreateUserPort,
        G: HrnGenerator,
    {
        CreateUserUseCase::new(persister, hrn_generator)
    }
}
