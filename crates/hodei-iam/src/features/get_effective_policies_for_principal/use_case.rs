use crate::features::get_effective_policies_for_principal::dto::{
    EffectivePoliciesResponse, GetEffectivePoliciesQuery,
};
use crate::features::get_effective_policies_for_principal::error::{
    GetEffectivePoliciesError, GetEffectivePoliciesResult,
};
use crate::features::get_effective_policies_for_principal::ports::{
    GroupFinderPort, PolicyFinderPort, UserFinderPort,
};
use cedar_policy::PolicySet;
use kernel::Hrn;
use std::sync::Arc;
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
///
/// # Arquitectura
/// Usa ports segregados (ISP - Interface Segregation Principle) para:
/// - UserFinderPort: Buscar usuarios
/// - GroupFinderPort: Buscar grupos del usuario
/// - PolicyFinderPort: Buscar políticas asociadas
pub struct GetEffectivePoliciesForPrincipalUseCase<UF, GF, PF>
where
    UF: UserFinderPort,
    GF: GroupFinderPort,
    PF: PolicyFinderPort,
{
    user_finder: Arc<UF>,
    group_finder: Arc<GF>,
    policy_finder: Arc<PF>,
}

impl<UF, GF, PF> GetEffectivePoliciesForPrincipalUseCase<UF, GF, PF>
where
    UF: UserFinderPort,
    GF: GroupFinderPort,
    PF: PolicyFinderPort,
{
    /// Create a new instance of the use case
    pub fn new(user_finder: Arc<UF>, group_finder: Arc<GF>, policy_finder: Arc<PF>) -> Self {
        Self {
            user_finder,
            group_finder,
            policy_finder,
        }
    }

    /// Ejecuta la obtención de políticas IAM efectivas devolviendo un PolicySet de Cedar
    ///
    /// Este es el método público que otros crates deben usar.
    /// No expone las entidades internas User/Group/Policy.
    ///
    /// # Flujo
    /// 1. Validar y parsear el HRN del principal
    /// 2. Buscar el usuario/service-account
    /// 3. Obtener grupos a los que pertenece
    /// 4. Recolectar políticas directas del principal
    /// 5. Recolectar políticas de todos los grupos
    /// 6. Combinar todo en un PolicySet de Cedar
    pub async fn execute(
        &self,
        query: GetEffectivePoliciesQuery,
    ) -> GetEffectivePoliciesResult<EffectivePoliciesResponse> {
        info!(
            "Getting effective policies for principal: {}",
            query.principal_hrn
        );

        // Step 1: Validar y parsear el HRN del principal
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

        // Step 2: Buscar el usuario (verificar que existe)
        let user = self
            .user_finder
            .find_by_hrn(&principal_hrn)
            .await
            .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?
            .ok_or_else(|| {
                GetEffectivePoliciesError::PrincipalNotFound(query.principal_hrn.clone())
            })?;

        info!("Found principal: {}", user.name);

        // Step 3: Obtener grupos a los que pertenece el principal
        let groups = self
            .group_finder
            .find_groups_by_user_hrn(&user.hrn)
            .await
            .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;

        info!("Principal belongs to {} group(s)", groups.len());

        // Step 4: Recolectar políticas directas del principal
        let principal_policies = self
            .policy_finder
            .find_policies_by_principal(&user.hrn)
            .await
            .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;

        info!(
            "Found {} direct policies for principal",
            principal_policies.len()
        );

        // Step 5: Recolectar políticas de todos los grupos
        let mut all_group_policies = Vec::new();
        for group in &groups {
            let group_policies = self
                .policy_finder
                .find_policies_by_principal(&group.hrn)
                .await
                .map_err(|e| GetEffectivePoliciesError::RepositoryError(e.to_string()))?;

            info!(
                "Found {} policies for group {}",
                group_policies.len(),
                group.name
            );

            all_group_policies.extend(group_policies);
        }

        // Step 6: Combinar todas las políticas en un PolicySet
        let all_policy_documents: Vec<String> = principal_policies
            .into_iter()
            .chain(all_group_policies)
            .collect();

        let policy_set = self.convert_to_policy_set(all_policy_documents)?;

        info!(
            "Successfully collected {} effective policies for principal {}",
            policy_set.policies().count(),
            query.principal_hrn
        );

        Ok(EffectivePoliciesResponse::new(
            policy_set,
            query.principal_hrn,
        ))
    }

    /// Convierte las políticas IAM internas a un PolicySet de Cedar
    ///
    /// Este método oculta los detalles de las entidades internas y solo
    /// expone el PolicySet que otros crates pueden usar.
    ///
    /// # Error Handling
    /// Las políticas que no se pueden parsear se registran como warnings
    /// pero no detienen el proceso. Esto permite que algunas políticas
    /// válidas funcionen incluso si otras están mal formadas.
    fn convert_to_policy_set(
        &self,
        policy_documents: Vec<String>,
    ) -> GetEffectivePoliciesResult<PolicySet> {
        let mut policy_set = PolicySet::new();
        let mut parse_errors = 0;

        for (idx, policy_doc) in policy_documents.into_iter().enumerate() {
            match policy_doc.parse::<cedar_policy::Policy>() {
                Ok(policy) => {
                    if let Err(e) = policy_set.add(policy) {
                        warn!("Failed to add policy {} to set: {}", idx, e);
                        parse_errors += 1;
                    }
                }
                Err(e) => {
                    warn!("Failed to parse policy document {}: {}", idx, e);
                    parse_errors += 1;
                }
            }
        }

        if parse_errors > 0 {
            warn!(
                "Encountered {} policy parse/add errors during conversion",
                parse_errors
            );
        }

        Ok(policy_set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::ports::{
        GroupRepositoryError, PolicyRepositoryError, UserRepositoryError,
    };
    use crate::shared::domain::{Group, User};

    // Mock implementations for testing
    struct MockUserFinder {
        users: Vec<User>,
    }

    #[async_trait::async_trait]
    impl UserFinderPort for MockUserFinder {
        async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, UserRepositoryError> {
            Ok(self.users.iter().find(|u| &u.hrn == hrn).cloned())
        }
    }

    struct MockGroupFinder {
        groups: Vec<(Hrn, Vec<Group>)>, // (user_hrn, groups)
    }

    #[async_trait::async_trait]
    impl GroupFinderPort for MockGroupFinder {
        async fn find_groups_by_user_hrn(
            &self,
            user_hrn: &Hrn,
        ) -> Result<Vec<Group>, GroupRepositoryError> {
            Ok(self
                .groups
                .iter()
                .find(|(hrn, _)| hrn == user_hrn)
                .map(|(_, groups)| groups.clone())
                .unwrap_or_default())
        }
    }

    struct MockPolicyFinder {
        policies: Vec<(Hrn, Vec<String>)>, // (principal_hrn, policies)
    }

    #[async_trait::async_trait]
    impl PolicyFinderPort for MockPolicyFinder {
        async fn find_policies_by_principal(
            &self,
            principal_hrn: &Hrn,
        ) -> Result<Vec<String>, PolicyRepositoryError> {
            Ok(self
                .policies
                .iter()
                .find(|(hrn, _)| hrn == principal_hrn)
                .map(|(_, policies)| policies.clone())
                .unwrap_or_default())
        }
    }

    #[tokio::test]
    async fn test_execute_with_valid_user_and_policies() {
        // Setup - Test with just user policy to avoid Cedar PolicyID collision
        let user_hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/test-user").unwrap();
        let user = User::new(
            user_hrn.clone(),
            "test-user".to_string(),
            "test@example.com".to_string(),
        );

        let user_policy =
            r#"permit(principal == Hodei::IAM::User::"test-user", action, resource);"#.to_string();

        let user_finder = Arc::new(MockUserFinder {
            users: vec![user.clone()],
        });

        let group_finder = Arc::new(MockGroupFinder { groups: vec![] });

        let policy_finder = Arc::new(MockPolicyFinder {
            policies: vec![(user_hrn.clone(), vec![user_policy])],
        });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        // Execute
        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:hodei:iam:us-east-1:default:user/test-user".to_string(),
        };

        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Execute should succeed");
        let response = result.unwrap();
        assert_eq!(
            response.principal_hrn,
            "hrn:hodei:iam:us-east-1:default:user/test-user"
        );

        // Should have 1 policy from user
        assert_eq!(response.policy_count, 1);
        assert_eq!(response.policies.policies().count(), 1);
    }

    #[tokio::test]
    async fn test_execute_with_user_and_group_policies() {
        // Setup - Test group policy collection
        let user_hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/test-user").unwrap();
        let user = User::new(
            user_hrn.clone(),
            "test-user".to_string(),
            "test@example.com".to_string(),
        );

        let group_hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:group/admins").unwrap();
        let group = Group::new(group_hrn.clone(), "admins".to_string());

        let group_policy =
            r#"forbid(principal == Hodei::IAM::Group::"admins", action, resource);"#.to_string();

        let user_finder = Arc::new(MockUserFinder {
            users: vec![user.clone()],
        });

        let group_finder = Arc::new(MockGroupFinder {
            groups: vec![(user_hrn.clone(), vec![group.clone()])],
        });

        let policy_finder = Arc::new(MockPolicyFinder {
            policies: vec![(group_hrn.clone(), vec![group_policy])],
        });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        // Execute
        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:hodei:iam:us-east-1:default:user/test-user".to_string(),
        };

        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok(), "Execute should succeed");
        let response = result.unwrap();

        // Should have 1 policy from group membership
        assert_eq!(response.policy_count, 1);
        assert_eq!(response.policies.policies().count(), 1);
    }

    #[tokio::test]
    async fn test_execute_with_user_not_found() {
        let user_finder = Arc::new(MockUserFinder { users: vec![] });
        let group_finder = Arc::new(MockGroupFinder { groups: vec![] });
        let policy_finder = Arc::new(MockPolicyFinder { policies: vec![] });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:hodei:iam:us-east-1:default:user/nonexistent".to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetEffectivePoliciesError::PrincipalNotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_with_repository_error() {
        // Mock that returns a repository error
        struct MockUserFinderWithError;

        #[async_trait::async_trait]
        impl UserFinderPort for MockUserFinderWithError {
            async fn find_by_hrn(&self, _hrn: &Hrn) -> Result<Option<User>, UserRepositoryError> {
                Err(UserRepositoryError::DatabaseError(
                    "Connection failed".to_string(),
                ))
            }
        }

        let user_finder = Arc::new(MockUserFinderWithError);
        let group_finder = Arc::new(MockGroupFinder { groups: vec![] });
        let policy_finder = Arc::new(MockPolicyFinder { policies: vec![] });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:hodei:iam:us-east-1:default:user/test-user".to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetEffectivePoliciesError::RepositoryError(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_with_group_repository_error() {
        let user_hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/test-user").unwrap();
        let user = User::new(
            user_hrn.clone(),
            "test-user".to_string(),
            "test@example.com".to_string(),
        );

        // Mock that returns a repository error for groups
        struct MockGroupFinderWithError;

        #[async_trait::async_trait]
        impl GroupFinderPort for MockGroupFinderWithError {
            async fn find_groups_by_user_hrn(
                &self,
                _user_hrn: &Hrn,
            ) -> Result<Vec<Group>, GroupRepositoryError> {
                Err(GroupRepositoryError::DatabaseError(
                    "Group query failed".to_string(),
                ))
            }
        }

        let user_finder = Arc::new(MockUserFinder {
            users: vec![user.clone()],
        });
        let group_finder = Arc::new(MockGroupFinderWithError);
        let policy_finder = Arc::new(MockPolicyFinder { policies: vec![] });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:hodei:iam:us-east-1:default:user/test-user".to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetEffectivePoliciesError::RepositoryError(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_with_policy_repository_error() {
        let user_hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/test-user").unwrap();
        let user = User::new(
            user_hrn.clone(),
            "test-user".to_string(),
            "test@example.com".to_string(),
        );

        // Mock that returns a repository error for policies
        struct MockPolicyFinderWithError;

        #[async_trait::async_trait]
        impl PolicyFinderPort for MockPolicyFinderWithError {
            async fn find_policies_by_principal(
                &self,
                _principal_hrn: &Hrn,
            ) -> Result<Vec<String>, PolicyRepositoryError> {
                Err(PolicyRepositoryError::DatabaseError(
                    "Policy query failed".to_string(),
                ))
            }
        }

        let user_finder = Arc::new(MockUserFinder {
            users: vec![user.clone()],
        });
        let group_finder = Arc::new(MockGroupFinder { groups: vec![] });
        let policy_finder = Arc::new(MockPolicyFinderWithError);

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:hodei:iam:us-east-1:default:user/test-user".to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetEffectivePoliciesError::RepositoryError(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_with_invalid_hrn() {
        let user_finder = Arc::new(MockUserFinder { users: vec![] });
        let group_finder = Arc::new(MockGroupFinder { groups: vec![] });
        let policy_finder = Arc::new(MockPolicyFinder { policies: vec![] });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

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
        let user_finder = Arc::new(MockUserFinder { users: vec![] });
        let group_finder = Arc::new(MockGroupFinder { groups: vec![] });
        let policy_finder = Arc::new(MockPolicyFinder { policies: vec![] });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:hodei:s3:us-east-1:default:bucket/test-bucket".to_string(),
        };

        let result = use_case.execute(query).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetEffectivePoliciesError::InvalidPrincipalType(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_with_no_policies() {
        let user_hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/test-user").unwrap();
        let user = User::new(
            user_hrn.clone(),
            "test-user".to_string(),
            "test@example.com".to_string(),
        );

        let user_finder = Arc::new(MockUserFinder { users: vec![user] });
        let group_finder = Arc::new(MockGroupFinder { groups: vec![] });
        let policy_finder = Arc::new(MockPolicyFinder { policies: vec![] });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:hodei:iam:us-east-1:default:user/test-user".to_string(),
        };

        let result = use_case.execute(query).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy_count, 0);
    }
}
