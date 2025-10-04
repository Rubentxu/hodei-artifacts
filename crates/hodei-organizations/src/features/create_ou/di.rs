use crate::shared::application::ports::OuRepository;
use crate::features::create_ou::use_case::CreateOuUseCase;
use crate::features::create_ou::adapter::OuRepositoryAdapter;

/// Create an instance of the CreateOuUseCase with the provided repository
pub fn create_ou_use_case<OR: OuRepository>(
    ou_repository: OR,
) -> CreateOuUseCase<OuRepositoryAdapter<OR>> {
    let adapter = OuRepositoryAdapter::new(ou_repository);
    CreateOuUseCase::new(adapter)
}
