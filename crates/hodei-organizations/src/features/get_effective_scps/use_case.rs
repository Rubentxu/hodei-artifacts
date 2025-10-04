use crate::shared::domain::scp::ServiceControlPolicy;
use crate::features::get_effective_scps::error::GetEffectiveScpsError;
use crate::features::get_effective_scps::ports::{
    ScpRepositoryPort,
    OuRepositoryPort,
    AccountRepositoryPort,
};
use policies::shared::domain::hrn::Hrn;

/// Caso de uso para obtener las SCPs efectivas de una entidad (OU o Account)
pub struct GetEffectiveScpsUseCase<SRP, ORP>
where
    SRP: ScpRepositoryPort + Send + Sync,
    ORP: OuRepositoryPort + AccountRepositoryPort + Send + Sync,
{
    scp_repository: SRP,
    org_repository: ORP,
}

impl<SRP, ORP> GetEffectiveScpsUseCase<SRP, ORP>
where
    SRP: ScpRepositoryPort + Send + Sync,
    ORP: OuRepositoryPort + AccountRepositoryPort + Send + Sync,
{
    pub fn new(scp_repository: SRP, org_repository: ORP) -> Self {
        Self { scp_repository, org_repository }
    }

    /// Ejecuta la obtención de SCPs efectivas devolviendo la lista completa de objetos
    pub async fn execute(&self, target_hrn_str: String) -> Result<Vec<ServiceControlPolicy>, GetEffectiveScpsError> {
        let target_hrn = Hrn::from_string(&target_hrn_str)
            .ok_or_else(|| GetEffectiveScpsError::TargetNotFound(target_hrn_str.clone()))?;

        match target_hrn.resource_type.as_str() {
            // Para una OU devolvemos las SCPs directamente unidas a ella
            "ou" => self.collect_from_ou(&target_hrn).await,
            // Para una cuenta buscamos su OU padre y usamos sus SCPs (modelo actual simplificado)
            "account" => {
                if let Some(account) = self.org_repository.find_account_by_hrn(&target_hrn).await? {
                    self.collect_from_ou(&account.parent_hrn).await
                } else {
                    Err(GetEffectiveScpsError::TargetNotFound(target_hrn_str))
                }
            }
            other => Err(GetEffectiveScpsError::InvalidTargetType(other.to_string())),
        }
    }

    async fn collect_from_ou(&self, ou_hrn: &Hrn) -> Result<Vec<ServiceControlPolicy>, GetEffectiveScpsError> {
        let ou = self.org_repository.find_ou_by_hrn(ou_hrn).await?
            .ok_or_else(|| GetEffectiveScpsError::TargetNotFound(ou_hrn.to_string()))?;

        let mut scps = Vec::new();
        for scp_hrn in ou.attached_scps.iter() {
            if let Some(scp) = self.scp_repository.find_scp_by_hrn(scp_hrn).await? {
                scps.push(scp);
            }
        }
        Ok(scps)
    }
}
