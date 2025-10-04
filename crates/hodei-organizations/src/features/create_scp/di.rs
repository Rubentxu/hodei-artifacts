use crate::shared::application::ports::ScpRepository;
use crate::features::create_scp::use_case::CreateScpUseCase;
use crate::features::create_scp::adapter::ScpRepositoryAdapter;

/// Create an instance of the CreateScpUseCase with the provided repository
pub fn create_scp_use_case<SR: ScpRepository>(
    scp_repository: SR,
) -> CreateScpUseCase<ScpRepositoryAdapter<SR>> {
    let adapter = ScpRepositoryAdapter::new(scp_repository);
    CreateScpUseCase::new(adapter)
}
