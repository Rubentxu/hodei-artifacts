use crate::shared::application::ports::scp_repository::ScpRepository;
use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::application::ports::ou_repository::OuRepository;
use crate::features::attach_scp::use_case::AttachScpUseCase;
use crate::features::attach_scp::adapter::{ScpRepositoryAdapter, AccountRepositoryAdapter, OuRepositoryAdapter};

/// Create an instance of the AttachScpUseCase with the provided repositories
pub fn attach_scp_use_case<SR: ScpRepository + std::marker::Sync + std::marker::Send, AR: AccountRepository + std::marker::Sync + std::marker::Send, OR: OuRepository + std::marker::Sync + std::marker::Send>(
    scp_repository: SR,
    account_repository: AR,
    ou_repository: OR,
) -> AttachScpUseCase<ScpRepositoryAdapter<SR>, AccountRepositoryAdapter<AR>, OuRepositoryAdapter<OR>> {
    let scp_adapter = ScpRepositoryAdapter::new(scp_repository);
    let account_adapter = AccountRepositoryAdapter::new(account_repository);
    let ou_adapter = OuRepositoryAdapter::new(ou_repository);
    AttachScpUseCase::new(scp_adapter, account_adapter, ou_adapter)
}
