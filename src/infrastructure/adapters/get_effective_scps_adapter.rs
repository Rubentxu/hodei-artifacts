//! Adaptador que conecta el caso de uso GetEffectiveScpsUseCase con el puerto del kernel.
//!
//! Este adaptador es parte de la composition root y NO debe estar en la API pública
//! del bounded context hodei-organizations.

use async_trait::async_trait;
use hodei_organizations::{GetEffectiveScpsQuery, GetEffectiveScpsUseCase, EffectiveScpsResponse};
use kernel::{GetEffectiveScpsPort, GetEffectiveScpsQuery as KernelQuery};

/// Adaptador que implementa GetEffectiveScpsPort del kernel wrapeando el caso de uso
/// de organizations.
///
/// Este adaptador traduce entre los DTOs del kernel y los DTOs de hodei-organizations,
/// permitiendo que el authorizer use SCPs sin conocer detalles de implementación.
pub struct GetEffectiveScpsAdapter<ScpRepo, OrgRepo>
where
    ScpRepo: hodei_organizations::ports::ScpRepositoryPort + Send + Sync,
    OrgRepo: hodei_organizations::ports::OuRepositoryPort
        + hodei_organizations::ports::AccountRepositoryPort
        + Send
        + Sync,
{
    inner: GetEffectiveScpsUseCase<ScpRepo, OrgRepo>,
}

impl<ScpRepo, OrgRepo> GetEffectiveScpsAdapter<ScpRepo, OrgRepo>
where
    ScpRepo: hodei_organizations::ports::ScpRepositoryPort + Send + Sync,
    OrgRepo: hodei_organizations::ports::OuRepositoryPort
        + hodei_organizations::ports::AccountRepositoryPort
        + Send
        + Sync,
{
    /// Crea un nuevo adaptador wrapeando el caso de uso
    pub fn new(use_case: GetEffectiveScpsUseCase<ScpRepo, OrgRepo>) -> Self {
        Self { inner: use_case }
    }
}

#[async_trait]
impl<ScpRepo, OrgRepo> GetEffectiveScpsPort for GetEffectiveScpsAdapter<ScpRepo, OrgRepo>
where
    ScpRepo: hodei_organizations::ports::ScpRepositoryPort + Send + Sync,
    OrgRepo: hodei_organizations::ports::OuRepositoryPort
        + hodei_organizations::ports::AccountRepositoryPort
        + Send
        + Sync,
{
    async fn get_effective_scps(
        &self,
        query: KernelQuery,
    ) -> Result<cedar_policy::PolicySet, Box<dyn std::error::Error + Send + Sync>> {
        // Traducir del DTO del kernel al DTO de hodei-organizations
        let internal_query = GetEffectiveScpsQuery {
            resource_hrn: query.resource_hrn,
        };

        // Ejecutar el caso de uso
        let response = self
            .inner
            .execute(internal_query)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        // Retornar el PolicySet
        Ok(response.policies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::Hrn;

    // Mock simple para ScpRepositoryPort
    struct MockScpRepo;

    #[async_trait]
    impl hodei_organizations::ports::ScpRepositoryPort for MockScpRepo {
        async fn find_by_hrn(
            &self,
            _hrn: &Hrn,
        ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Some("permit(principal, action, resource);".to_string()))
        }
    }

    // Mock simple para OuRepositoryPort + AccountRepositoryPort
    struct MockOrgRepo;

    #[async_trait]
    impl hodei_organizations::ports::OuRepositoryPort for MockOrgRepo {
        async fn find_by_hrn(
            &self,
            _hrn: &Hrn,
        ) -> Result<Option<(Hrn, Vec<Hrn>)>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
    }

    #[async_trait]
    impl hodei_organizations::ports::AccountRepositoryPort for MockOrgRepo {
        async fn find_by_hrn(
            &self,
            _hrn: &Hrn,
        ) -> Result<Option<Hrn>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
    }

    #[tokio::test]
    async fn test_adapter_translates_queries() {
        // Arrange
        let scp_repo = MockScpRepo;
        let org_repo = MockOrgRepo;
        let use_case = GetEffectiveScpsUseCase::new(scp_repo, org_repo);
        let adapter = GetEffectiveScpsAdapter::new(use_case);

        let query = KernelQuery {
            resource_hrn: Hrn::new(
                "aws".into(),
                "organizations".into(),
                "123456789012".into(),
                "account".into(),
                "acc-123".into(),
            ),
        };

        // Act
        let result = adapter.get_effective_scps(query).await;

        // Assert
        assert!(result.is_ok(), "Adapter should successfully translate query");
    }
}

