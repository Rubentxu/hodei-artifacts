use crate::internal::application::ports::{AccountRepository, OuRepository};
use crate::features::move_account::use_case::MoveAccountUseCase;
use crate::features::move_account::adapter::{AccountRepositoryAdapter, OuRepositoryAdapter};

/// Create an instance of the MoveAccountUseCase with the provided repositories
pub fn move_account_use_case<AR: AccountRepository, OR: OuRepository>(
    account_repository: AR,
    ou_repository: OR,
) -> MoveAccountUseCase<AccountRepositoryAdapter<AR>, OuRepositoryAdapter<OR>> {
    let account_adapter = AccountRepositoryAdapter::new(account_repository);
    let ou_adapter = OuRepositoryAdapter::new(ou_repository);
    MoveAccountUseCase::new(account_adapter, ou_adapter)
}
