//! Use Case: Get Policy

use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info};

use super::dto::{GetPolicyQuery, PolicyView};
use super::error::GetPolicyError;
use super::ports::{GetPolicyUseCasePort, PolicyReader};
use kernel::Hrn;

/// Caso de uso: Obtener una política IAM por su HRN
pub struct GetPolicyUseCase {
    reader: Arc<dyn PolicyReader>,
}

impl GetPolicyUseCase {
    /// Crea una nueva instancia del caso de uso
    pub fn new(reader: Arc<dyn PolicyReader>) -> Self {
        Self { reader }
    }

    /// Ejecuta el caso de uso
    pub async fn execute(&self, query: GetPolicyQuery) -> Result<PolicyView, GetPolicyError> {
        info!("Getting policy: {}", query.policy_hrn);

        // Validar que el HRN sea de tipo Policy
        if query.policy_hrn.resource_type() != "Policy" {
            return Err(GetPolicyError::InvalidHrn(format!(
                "Expected Policy HRN, got: {}",
                query.policy_hrn.resource_type()
            )));
        }

        // Obtener la política usando el reader
        let policy = self.reader.get_by_hrn(&query.policy_hrn).await?;

        debug!("Policy retrieved successfully: {}", policy.hrn);

        Ok(policy)
    }
}

// Implement PolicyReader trait for the use case to enable trait object usage
#[async_trait]
impl PolicyReader for GetPolicyUseCase {
    async fn get_by_hrn(&self, hrn: &Hrn) -> Result<PolicyView, GetPolicyError> {
        let query = GetPolicyQuery {
            policy_hrn: hrn.clone(),
        };
        self.execute(query).await
    }
}

#[async_trait]
impl GetPolicyUseCasePort for GetPolicyUseCase {
    async fn execute(&self, query: GetPolicyQuery) -> Result<PolicyView, GetPolicyError> {
        self.execute(query).await
    }
}
