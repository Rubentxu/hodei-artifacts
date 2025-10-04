use crate::shared::domain::scp::ServiceControlPolicy;
use crate::features::create_scp::ports::ScpPersister;
use crate::features::create_scp::dto::{CreateScpCommand, ScpView};
use crate::features::create_scp::error::CreateScpError;
use policies::shared::domain::hrn::Hrn;
use std::sync::Arc;

pub struct CreateScpUseCase<SP: ScpPersister> {
    persister: Arc<SP>,
}

impl<SP: ScpPersister> CreateScpUseCase<SP> {
    pub fn new(persister: Arc<SP>) -> Self {
        Self { persister }
    }
    
    pub async fn execute(&self, command: CreateScpCommand) -> Result<ScpView, CreateScpError> {
        // Validar el nombre de la SCP
        if command.name.is_empty() {
            return Err(CreateScpError::InvalidScpName);
        }
        
        // Validar el documento de la SCP
        if command.document.is_empty() {
            return Err(CreateScpError::InvalidScpDocument);
        }
        
        // Crear el HRN para la SCP
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "scp".to_string(),
            command.name.clone(),
        );

        // Crear la SCP
        let scp = ServiceControlPolicy::new(scp_hrn, command.name.clone(), command.document.clone());
        
        // Guardar la SCP
        self.persister.save(scp.clone()).await?;
        
        // Devolver la vista de la SCP
        Ok(ScpView {
            hrn: scp.hrn,
            name: scp.name,
            document: scp.document,
        })
    }
}
