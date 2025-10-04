use crate::shared::application::ports::AccountRepository;
use crate::features::create_account::use_case::CreateAccountUseCase;
use crate::features::create_account::adapter::AccountRepositoryAdapter;

/// Create an instance of the CreateAccountUseCase with the provided repository
pub fn create_account_use_case<AR: AccountRepository>(
    account_repository: AR,
) -> CreateAccountUseCase<AccountRepositoryAdapter<AR>> {
    let adapter = AccountRepositoryAdapter::new(account_repository);
    CreateAccountUseCase::new(adapter)
}
