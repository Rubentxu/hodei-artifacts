use crate::features::get_effective_policies_for_principal::dto::{
    EffectivePoliciesResponse, GetEffectivePoliciesQuery,
};
use crate::features::get_effective_policies_for_principal::error::{
    GetEffectivePoliciesError, GetEffectivePoliciesResult,
};
use cedar_policy::PolicySet;
use policies::shared::domain::hrn::Hrn;
use tracing::{info, warn};

/// Caso de uso para obtener las políticas IAM efectivas de un principal
///
/// Este caso de uso es la ÚNICA forma de que otros crates accedan a las políticas IAM.
/// Devuelve un PolicySet de Cedar, NO las entidades internas User/Group/Policy.
///
/// # Responsabilidades
/// - Resolver el principal (User o ServiceAccount)
/// - Obtener grupos a los que pertenece el principal
/// - Recolectar políticas directas del principal
/// - Recolectar políticas de todos los grupos
/// - Combinar todo en un PolicySet de Cedar
pub struct GetEffectivePoliciesForPrincipalUseCase {
    // Los ports reales se inyectarán aquí cuando implementemos los repositorios
    // Por ahora es un placeholder para establecer el patrón
}

impl GetEffectivePoliciesForPrincipalUseCase {
    pub fn new() -> Self {
        Self {}
    }

    /// Ejecuta la obtención de políticas IAM efectivas devolviendo un PolicySet de Cedar
    ///
    /// Este es el método público que otros crates deben usar.
    /// No expone las entidades internas User/Group/Policy.
    pub async fn execute(
        &self,
        query: GetEffectivePoliciesQuery,
    ) -> GetEffectivePoliciesResult<EffectivePoliciesResponse> {
        info!(
            "Getting effective policies for principal: {}",
            query.principal_hrn
        );

        // Validar y parsear el HRN del principal
        let principal_hrn = Hrn::from_string(&query.principal_hrn).ok_or_else(|| {
            GetEffectivePoliciesError::InvalidPrincipalHrn(query.principal_hrn.clone())
        })?;

        // Validar que el tipo de recurso es válido para un principal
        let resource_type_lower = principal_hrn.resource_type.to_lowercase();
        match resource_type_lower.as_str() {
            "user" | "service-account" => {}
            _ => {
                return Err(GetEffectivePoliciesError::InvalidPrincipalType(
                    principal_hrn.resource_type.clone(),
                ));
            }
        }

        // TODO: Implementar la lógica real cuando tengamos los repositorios
        // Por ahora devolvemos un PolicySet vacío para establecer el contrato
        //
        // La implementación real haría:
        // 1. let user = self.user_repo.find_by_hrn(&principal_hrn).await?;
        // 2. let groups = self.group_repo.find_by_user(&user.hrn).await?;
        // 3. let user_policies = self.policy_repo.find_by_principal(&user.hrn).await?;
        // 4. let group_policies = for each group: self.policy_repo.find_by_principal(&group.hrn).await?;
        // 5. Combinar todas en un PolicySet

        let policy_set = PolicySet::new();

        info!(
            "Found {} effective policies for principal {}",
            policy_set.policies().count(),
            query.principal_hrn
        );

        Ok(EffectivePoliciesResponse::new(
            policy_set,
            query.principal_hrn,
        ))
    }

    /// Método interno (futuro) para obtener políticas directas del principal
    #[allow(dead_code)]
    async fn get_principal_policies(
        &self,
        _principal_hrn: &Hrn,
    ) -> GetEffectivePoliciesResult<Vec<String>> {
        // TODO: Implementar cuando tengamos repositorios
        Ok(vec![])
    }

    /// Método interno (futuro) para obtener políticas de grupos
    #[allow(dead_code)]
    async fn get_group_policies(
        &self,
        _principal_hrn: &Hrn,
    ) -> GetEffectivePoliciesResult<Vec<String>> {
        // TODO: Implementar cuando tengamos repositorios
        Ok(vec![])
    }

    /// Convierte las políticas IAM internas a un PolicySet de Cedar
    ///
    /// Este método oculta los detalles de las entidades internas y solo
    /// expone el PolicySet que otros crates pueden usar.
    #[allow(dead_code)]
    fn convert_to_policy_set(
        &self,
        policy_documents: Vec<String>,
    ) -> GetEffectivePoliciesResult<PolicySet> {
        let mut policy_set = PolicySet::new();

        for (idx, policy_doc) in policy_documents.into_iter().enumerate() {
            match policy_doc.parse::<cedar_policy::Policy>() {
                Ok(policy) => {
                    if let Err(e) = policy_set.add(policy) {
                        warn!("Failed to add policy {} to set: {}", idx, e);
                        // Continuamos con las demás políticas
                    }
                }
                Err(e) => {
                    warn!("Failed to parse policy document {}: {}", idx, e);
                    // Continuamos con las demás políticas
                }
            }
        }

        Ok(policy_set)
    }
}

impl Default for GetEffectivePoliciesForPrincipalUseCase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_with_valid_user_hrn() {
        let use_case = GetEffectivePoliciesForPrincipalUseCase::new();

        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:aws:iam:us-east-1:default:user/test-user".to_string(),
        };

        let result = use_case.execute(query).await;
        if let Err(e) = &result {
            eprintln!("Error: {:?}", e);
        }
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(
            response.principal_hrn,
            "hrn:aws:iam:us-east-1:default:user/test-user"
        );
        assert_eq!(response.policy_count, 0); // Empty for now
    }

    #[tokio::test]
    async fn test_execute_with_invalid_hrn() {
        let use_case = GetEffectivePoliciesForPrincipalUseCase::new();

        let query = GetEffectivePoliciesQuery {
            principal_hrn: "invalid-hrn".to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetEffectivePoliciesError::InvalidPrincipalHrn(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_with_invalid_principal_type() {
        let use_case = GetEffectivePoliciesForPrincipalUseCase::new();

        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:aws:s3:us-east-1:default:bucket/test-bucket".to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        eprintln!("Error received: {:?}", err);
        assert!(matches!(
            err,
            GetEffectivePoliciesError::InvalidPrincipalType(_)
        ));
    }
}
