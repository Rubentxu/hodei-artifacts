use crate::features::get_effective_scps::dto::{EffectiveScpsResponse, GetEffectiveScpsQuery};
use crate::features::get_effective_scps::error::GetEffectiveScpsError;
use crate::features::get_effective_scps::ports::{
    AccountRepositoryPort, OuRepositoryPort, ScpRepositoryPort,
};
use crate::shared::domain::scp::ServiceControlPolicy;
use cedar_policy::PolicySet;
use kernel::Hrn;
use tracing::{info, warn};

/// Caso de uso para obtener las SCPs efectivas de una entidad (OU o Account)
///
/// Este caso de uso es la ÚNICA forma de que otros crates accedan a las SCPs.
/// Devuelve un PolicySet de Cedar, NO las entidades internas ServiceControlPolicy.
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
        Self {
            scp_repository,
            org_repository,
        }
    }

    /// Ejecuta la obtención de SCPs efectivas devolviendo un PolicySet de Cedar
    ///
    /// Este es el método público que otros crates deben usar.
    /// No expone las entidades internas ServiceControlPolicy.
    pub async fn execute(
        &self,
        query: GetEffectiveScpsQuery,
    ) -> Result<EffectiveScpsResponse, GetEffectiveScpsError> {
        info!(
            "Getting effective SCPs for resource: {}",
            query.resource_hrn
        );

        let target_hrn = Hrn::from_string(&query.resource_hrn)
            .ok_or_else(|| GetEffectiveScpsError::TargetNotFound(query.resource_hrn.clone()))?;

        // Obtener las entidades SCP internas (no expuestas)
        let scps = match target_hrn.resource_type.as_str() {
            "ou" => self.collect_from_ou(&target_hrn).await?,
            "account" => {
                if let Some(account) = self.org_repository.find_account_by_hrn(&target_hrn).await? {
                    if let Some(parent_hrn) = &account.parent_hrn {
                        self.collect_from_ou(parent_hrn).await?
                    } else {
                        // Account without parent OU: no inherited SCPs
                        Vec::new()
                    }
                } else {
                    return Err(GetEffectiveScpsError::TargetNotFound(query.resource_hrn));
                }
            }
            other => return Err(GetEffectiveScpsError::InvalidTargetType(other.to_string())),
        };

        info!("Found {} effective SCPs", scps.len());

        // Convertir las entidades internas a PolicySet de Cedar
        let policy_set = self.convert_to_policy_set(scps)?;

        Ok(EffectiveScpsResponse::new(policy_set, query.resource_hrn))
    }

    /// Método interno para recolectar SCPs desde una OU
    async fn collect_from_ou(
        &self,
        ou_hrn: &Hrn,
    ) -> Result<Vec<ServiceControlPolicy>, GetEffectiveScpsError> {
        let ou = self
            .org_repository
            .find_ou_by_hrn(ou_hrn)
            .await?
            .ok_or_else(|| GetEffectiveScpsError::TargetNotFound(ou_hrn.to_string()))?;

        let mut scps = Vec::new();
        for scp_hrn in ou.attached_scps.iter() {
            if let Some(scp) = self.scp_repository.find_scp_by_hrn(scp_hrn).await? {
                scps.push(scp);
            } else {
                warn!("SCP referenced but not found: {}", scp_hrn);
            }
        }

        Ok(scps)
    }

    /// Convierte las entidades SCP internas a un PolicySet de Cedar
    ///
    /// Este método oculta los detalles de las entidades internas y solo
    /// expone el PolicySet que otros crates pueden usar.
    fn convert_to_policy_set(
        &self,
        scps: Vec<ServiceControlPolicy>,
    ) -> Result<PolicySet, GetEffectiveScpsError> {
        let mut policy_set = PolicySet::new();

        for scp in scps {
            // Convertir la política Cedar string a Policy
            match scp.document.parse::<cedar_policy::Policy>() {
                Ok(policy) => {
                    if let Err(e) = policy_set.add(policy) {
                        warn!("Failed to add SCP policy to set: {}", e);
                        // Continuamos con las demás políticas
                    }
                }
                Err(e) => {
                    warn!("Failed to parse SCP policy document for {}: {}", scp.hrn, e);
                    // Continuamos con las demás políticas
                }
            }
        }

        Ok(policy_set)
    }
}
