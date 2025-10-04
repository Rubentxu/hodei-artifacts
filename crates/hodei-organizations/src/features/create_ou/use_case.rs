use crate::shared::domain::ou::OrganizationalUnit;
use crate::features::create_ou::ports::OuPersister;
use crate::features::create_ou::dto::{CreateOuCommand, OuView};
use crate::features::create_ou::error::CreateOuError;
use std::sync::Arc;

pub struct CreateOuUseCase<OP: OuPersister> {
    persister: Arc<OP>,
}

impl<OP: OuPersister> CreateOuUseCase<OP> {
    pub fn new(persister: Arc<OP>) -> Self {
        Self { persister }
    }
    
    pub async fn execute(&self, command: CreateOuCommand) -> Result<OuView, CreateOuError> {
        // Validar el nombre de la OU
        if command.name.is_empty() {
            return Err(CreateOuError::InvalidOuName);
        }
        
        // Crear la OU
        let ou = OrganizationalUnit::new(command.name.clone(), command.parent_hrn.clone());
        
        // Guardar la OU
        self.persister.save(ou.clone()).await?;
        
        // Devolver la vista de la OU
        Ok(OuView {
            hrn: ou.hrn,
            name: ou.name,
            parent_hrn: ou.parent_hrn,
        })
    }
}
