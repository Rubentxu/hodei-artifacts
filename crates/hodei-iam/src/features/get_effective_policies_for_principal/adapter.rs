//! Adaptadores para el caso de uso get_effective_policies_for_principal
//!
//! Estos adaptadores conectan los ports segregados del caso de uso con
//! los repositorios de la capa de aplicación compartida.

use crate::features::get_effective_policies_for_principal::ports::{
    GroupFinderPort, PolicyFinderPort, UserFinderPort,
};
use crate::internal::application::ports::{GroupRepository, UserRepository};
use crate::internal::domain::{Group, User};
use kernel::Hrn;
use std::sync::Arc;

/// Adaptador que conecta UserFinderPort con UserRepository
pub struct UserFinderAdapter<UR: UserRepository> {
    repository: Arc<UR>,
}

impl<UR: UserRepository> UserFinderAdapter<UR> {
    pub fn new(repository: Arc<UR>) -> Self {
        Self { repository }
    }
}

#[async_trait::async_trait]
impl<UR: UserRepository> UserFinderPort for UserFinderAdapter<UR> {
    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>> {
        self.repository
            .find_by_hrn(hrn)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }
}

/// Adaptador que conecta GroupFinderPort con GroupRepository
pub struct GroupFinderAdapter<GR: GroupRepository> {
    repository: Arc<GR>,
}

impl<GR: GroupRepository> GroupFinderAdapter<GR> {
    pub fn new(repository: Arc<GR>) -> Self {
        Self { repository }
    }
}

#[async_trait::async_trait]
impl<GR: GroupRepository> GroupFinderPort for GroupFinderAdapter<GR> {
    async fn find_groups_by_user_hrn(
        &self,
        _user_hrn: &Hrn,
    ) -> Result<Vec<Group>, Box<dyn std::error::Error + Send + Sync>> {
        // TODO: Implementar cuando GroupRepository tenga find_groups_by_user_hrn
        // Por ahora, obtener todos los grupos y filtrar por membresía
        let all_groups = self
            .repository
            .find_all()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        // Filtrar grupos donde el usuario es miembro
        let user_groups: Vec<Group> = all_groups
            .into_iter()
            .filter(|group| {
                // Verificar si el usuario está en la lista de miembros del grupo
                // Asumiendo que Group tiene alguna forma de verificar membresía
                group.attached_policy_hrns.iter().any(|_| {
                    // TODO: Esta lógica necesita ser implementada correctamente
                    // cuando tengamos la estructura de datos de membresía
                    false
                })
            })
            .collect();

        Ok(user_groups)
    }
}

/// Adaptador que conecta PolicyFinderPort con búsqueda de políticas
///
/// NOTA: Las políticas IAM no se almacenan en hodei-iam, sino que se
/// obtienen desde el authorization engine o el crate policies.
/// Este adapter actúa como un puente temporal hasta que se implemente
/// la integración completa.
pub struct PolicyFinderAdapter;

impl PolicyFinderAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PolicyFinderAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl PolicyFinderPort for PolicyFinderAdapter {
    async fn find_policies_by_principal(
        &self,
        _principal_hrn: &Hrn,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        // TODO: Implementar integración con el motor de políticas
        // Por ahora devolvemos un vector vacío
        // En una implementación real, esto consultaría:
        // 1. Políticas directamente adjuntas al principal
        // 2. Políticas heredadas de grupos
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::infrastructure::persistence::InMemoryUserRepository;

    #[tokio::test]
    async fn test_user_finder_adapter() {
        let repo = Arc::new(InMemoryUserRepository::new());
        let adapter = UserFinderAdapter::new(repo.clone());

        // Create a test user
        let user_hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/test").unwrap();
        let user = User::new(
            user_hrn.clone(),
            "test".to_string(),
            "test@example.com".to_string(),
        );

        repo.save(&user).await.unwrap();

        // Test finding the user
        let found = adapter.find_by_hrn(&user_hrn).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "test");
    }

    #[tokio::test]
    async fn test_user_finder_adapter_not_found() {
        let repo = Arc::new(InMemoryUserRepository::new());
        let adapter = UserFinderAdapter::new(repo);

        let user_hrn =
            Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/nonexistent").unwrap();

        let found = adapter.find_by_hrn(&user_hrn).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_policy_finder_adapter_returns_empty() {
        let adapter = PolicyFinderAdapter::new();

        let principal_hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/test").unwrap();

        let policies = adapter
            .find_policies_by_principal(&principal_hrn)
            .await
            .unwrap();
        assert!(policies.is_empty());
    }
}
