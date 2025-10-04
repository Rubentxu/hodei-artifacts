use crate::shared::domain::account::Account;
use crate::features::create_account::ports::AccountPersister;
use crate::features::create_account::dto::{CreateAccountCommand, AccountView};
use crate::features::create_account::error::CreateAccountError;
use std::sync::Arc;

pub struct CreateAccountUseCase<AP: AccountPersister> {
    persister: Arc<AP>,
}

impl<AP: AccountPersister> CreateAccountUseCase<AP> {
    pub fn new(persister: Arc<AP>) -> Self {
        Self { persister }
    }
    
    pub async fn execute(&self, command: CreateAccountCommand) -> Result<AccountView, CreateAccountError> {
        // Validar el nombre de la cuenta
        if command.name.is_empty() {
            return Err(CreateAccountError::InvalidAccountName);
        }
        
        // Crear la cuenta
        let account = Account::new(command.hrn, command.name.clone(), command.parent_hrn);
        
        // Guardar la cuenta
        self.persister.save(account.clone()).await?;
        
        // Devolver la vista de la cuenta
        Ok(AccountView {
            hrn: account.hrn,
            name: account.name,
            parent_hrn: account.parent_hrn,
        })
    }
}
