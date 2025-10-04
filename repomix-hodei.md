This file is a merged representation of a subset of the codebase, containing specifically included files, combined into a single document by Repomix.

<file_summary>
This section contains a summary of this file.

<purpose>
This file contains a packed representation of a subset of the repository's contents that is considered the most important context.
It is designed to be easily consumable by AI systems for analysis, code review,
or other automated processes.
</purpose>

<file_format>
The content is organized as follows:
1. This summary section
2. Repository information
3. Directory structure
4. Repository files (if enabled)
5. Multiple file entries, each consisting of:
  - File path as an attribute
  - Full contents of the file
</file_format>

<usage_guidelines>
- This file should be treated as read-only. Any changes should be made to the
  original repository files, not this packed version.
- When processing this file, use the file path to distinguish
  between different files in the repository.
- Be aware that this file may contain sensitive information. Handle it with
  the same level of security as you would the original repository.
</usage_guidelines>

<notes>
- Some files may have been excluded based on .gitignore rules and Repomix's configuration
- Binary files are not included in this packed representation. Please refer to the Repository Structure section for a complete list of file paths, including binary files
- Only files matching these patterns are included: crates/policies/**/*, crates/hodei-iam/**/*, crates/hodei-organizations/**/*, crates/hodei-authorizer/**/*, crates/shared/**/*
- Files matching patterns in .gitignore are excluded
- Files matching default ignore patterns are excluded
- Files are sorted by Git change count (files with more changes are at the bottom)
</notes>

</file_summary>

<directory_structure>
crates/
  hodei-authorizer/
    src/
      features/
        evaluate_permissions/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case.rs
        mod.rs
      application.rs
      authorizer.rs
      lib.rs
    Cargo.toml
    README.md
  hodei-iam/
    src/
      features/
        add_user_to_group/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        create_group/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        create_user/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        get_effective_policies_for_principal/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mod.rs
          ports.rs
          use_case.rs
        mod.rs
      shared/
        application/
          ports/
            mod.rs
          di_configurator.rs
          mod.rs
        domain/
          actions.rs
          entities.rs
          events.rs
          mod.rs
        infrastructure/
          persistence/
            mod.rs
          surreal/
            group_repository.rs
            iam_policy_provider.rs
            mod.rs
            policy_repository.rs
            user_repository.rs
          mod.rs
        mod.rs
      lib.rs
    tests/
      add_user_to_group_integration_test.rs
      create_user_integration_test.rs
      integration_add_user_to_group_comprehensive_test.rs
      integration_create_user_comprehensive_test.rs
      unit_group_test.rs
      unit_hrn_constructor_test.rs
      unit_user_test.rs
    Cargo.toml
  hodei-organizations/
    src/
      features/
        attach_scp/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case_test.rs
          use_case.rs
        create_account/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case_test.rs
          use_case.rs
        create_ou/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case_test.rs
          use_case.rs
        create_scp/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case_test.rs
          use_case.rs
        get_effective_scps/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case_test.rs
          use_case.rs
        move_account/
          adapter.rs
          di.rs
          dto.rs
          error.rs
          mocks.rs
          mod.rs
          ports.rs
          use_case_test.rs
          use_case.rs
        mod.rs
      shared/
        application/
          ports/
            account_repository.rs
            mod.rs
            ou_repository.rs
            scp_repository.rs
          hierarchy_service.rs
          mod.rs
        domain/
          account.rs
          events.rs
          mod.rs
          ou_test.rs
          ou.rs
          scp_test.rs
          scp.rs
        infrastructure/
          surreal/
            account_repository.rs
            mod.rs
            organization_boundary_provider.rs
            ou_repository.rs
            scp_repository.rs
            unit_of_work.rs
          mod.rs
        mod.rs
      lib.rs
    Cargo.toml
    README.md
  policies/
    src/
      features/
        batch_eval/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        create_policy/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        delete_policy/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        get_policy/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        list_policies/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        policy_analysis/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        policy_playground/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        policy_playground_traces/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        update_policy/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        validate_policy/
          di.rs
          dto.rs
          mod.rs
          use_case.rs
        mod.rs
      shared/
        application/
          di_helpers.rs
          engine.rs
          mod.rs
          parallel.rs
          store.rs
        domain/
          entity_utils.rs
          error.rs
          hrn.rs
          mod.rs
          ports.rs
          schema_assembler.rs
        infrastructure/
          surreal/
            embedded_storage.rs
            mem_storage.rs
            mod.rs
          mod.rs
        mod.rs
      lib.rs
    tests/
      delete_policy_integration_test.rs
      domain_compilation_test.rs
      hodei_entity_test.rs
      list_policies_integration_test.rs
      principals_schema_test.rs
      schema_rendering_final_test.rs
      shared_parallel_test.rs
      test_schema.rs
    Cargo.toml
  shared/
    src/
      application/
        ports/
          event_bus.rs
          mod.rs
          unit_of_work.rs
        mod.rs
      infrastructure/
        audit/
          handler_test.rs
          handler.rs
          mod.rs
          query_test.rs
          query.rs
        in_memory_event_bus.rs
        mod.rs
        surrealdb_adapter.rs
      enums.rs
      events.rs
      lib.rs
      lifecycle.rs
      models.rs
    Cargo.toml
    README.md
</directory_structure>

<files>
This section contains the contents of the repository's files.

<file path="crates/hodei-authorizer/src/features/evaluate_permissions/dto.rs">
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use policies::shared::domain::hrn::Hrn;

/// Request for authorization evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationRequest {
    /// The principal (user/service) requesting access
    pub principal: Hrn,
    /// The action being requested (e.g., "read", "write", "delete")
    pub action: String,
    /// The resource being accessed
    pub resource: Hrn,
    /// Additional context for the evaluation (optional)
    pub context: Option<AuthorizationContext>,
}

/// Additional context for authorization decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationContext {
    /// IP address of the requester
    pub source_ip: Option<String>,
    /// User agent string
    pub user_agent: Option<String>,
    /// Time of the request
    pub request_time: Option<time::OffsetDateTime>,
    /// Additional key-value context
    pub additional_context: HashMap<String, serde_json::Value>,
}

/// Response from authorization evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationResponse {
    /// The authorization decision
    pub decision: AuthorizationDecision,
    /// Policies that determined the decision
    pub determining_policies: Vec<String>,
    /// Reason for the decision
    pub reason: String,
    /// Whether the decision was explicit or implicit
    pub explicit: bool,
}

/// Authorization decision outcomes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthorizationDecision {
    /// Access is explicitly allowed
    Allow,
    /// Access is explicitly denied
    Deny,
}

/// Information about a policy that influenced the decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyImpact {
    /// ID of the policy
    pub policy_id: String,
    /// Name of the policy
    pub policy_name: String,
    /// Effect of this policy (Allow/Deny)
    pub effect: AuthorizationDecision,
    /// Whether this was a determining policy
    pub determining: bool,
}

impl Default for AuthorizationContext {
    fn default() -> Self {
        Self {
            source_ip: None,
            user_agent: None,
            request_time: Some(time::OffsetDateTime::now_utc()),
            additional_context: HashMap::new(),
        }
    }
}

impl AuthorizationRequest {
    /// Create a new authorization request
    pub fn new(principal: Hrn, action: String, resource: Hrn) -> Self {
        Self {
            principal,
            action,
            resource,
            context: None,
        }
    }

    /// Create a new authorization request with context
    pub fn with_context(
        principal: Hrn,
        action: String,
        resource: Hrn,
        context: AuthorizationContext,
    ) -> Self {
        Self {
            principal,
            action,
            resource,
            context: Some(context),
        }
    }
}

impl AuthorizationResponse {
    /// Create an allow response
    pub fn allow(policies: Vec<String>, reason: String) -> Self {
        Self {
            decision: AuthorizationDecision::Allow,
            determining_policies: policies,
            reason,
            explicit: true,
        }
    }

    /// Create a deny response
    pub fn deny(policies: Vec<String>, reason: String) -> Self {
        Self {
            decision: AuthorizationDecision::Deny,
            determining_policies: policies,
            reason,
            explicit: true,
        }
    }

    /// Create an implicit deny response (no policies matched)
    pub fn implicit_deny(reason: String) -> Self {
        Self {
            decision: AuthorizationDecision::Deny,
            determining_policies: vec![],
            reason,
            explicit: false,
        }
    }
}
</file>

<file path="crates/hodei-authorizer/src/features/evaluate_permissions/error.rs">
use thiserror::Error;

/// Errors specific to the evaluate permissions feature
#[derive(Debug, Error, Clone)]
pub enum EvaluatePermissionsError {
    #[error("Invalid authorization request: {0}")]
    InvalidRequest(String),

    #[error("Policy evaluation failed: {0}")]
    PolicyEvaluationFailed(String),

    #[error("IAM policy provider error: {0}")]
    IamPolicyProviderError(String),

    #[error("Organization boundary provider error: {0}")]
    OrganizationBoundaryProviderError(String),

    #[error("Cedar policy engine error: {0}")]
    CedarEngineError(String),

    #[error("Policy parsing error: {0}")]
    PolicyParsingError(String),

    #[error("Entity resolution error: {0}")]
    EntityResolutionError(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Timeout during authorization evaluation")]
    EvaluationTimeout,

    #[error("Internal authorization error: {0}")]
    InternalError(String),
}

impl From<crate::features::ports::AuthorizationError> for EvaluatePermissionsError {
    fn from(err: crate::features::ports::AuthorizationError) -> Self {
        match err {
            crate::features::ports::AuthorizationError::IamPolicyProvider(msg) => {
                EvaluatePermissionsError::IamPolicyProviderError(msg)
            }
            crate::features::ports::AuthorizationError::OrganizationBoundaryProvider(msg) => {
                EvaluatePermissionsError::OrganizationBoundaryProviderError(msg)
            }
        }
    }
}

impl From<cedar_policy::PolicySetError> for EvaluatePermissionsError {
    fn from(err: cedar_policy::PolicySetError) -> Self {
        EvaluatePermissionsError::PolicyParsingError(err.to_string())
    }
}

impl From<cedar_policy::ValidationError> for EvaluatePermissionsError {
    fn from(err: cedar_policy::ValidationError) -> Self {
        EvaluatePermissionsError::PolicyParsingError(err.to_string())
    }
}

/// Result type for evaluate permissions operations
pub type EvaluatePermissionsResult<T> = Result<T, EvaluatePermissionsError>;
</file>

<file path="crates/hodei-authorizer/src/features/mod.rs">
//! Features module for the hodei-authorizer crate
//! 
//! This module contains all authorization-related features organized
//! according to Vertical Slice Architecture principles.

pub mod evaluate_permissions;

// Re-export all features for easier access
pub use evaluate_permissions::*;
</file>

<file path="crates/hodei-authorizer/src/application.rs">
pub mod evaluate_permissions;
</file>

<file path="crates/hodei-authorizer/src/authorizer.rs">
use crate::ports::{IamPolicyProvider, OrganizationBoundaryProvider, AuthorizationError};

use policies::shared::domain::hrn::Hrn;


/// Authorizer service that combines IAM policies and SCPs to make authorization decisions
pub struct AuthorizerService<IAM: IamPolicyProvider, ORG: OrganizationBoundaryProvider> {
    iam_provider: IAM,
    org_provider: ORG,
    policy_evaluator: PolicyEvaluator,
}

impl<IAM: IamPolicyProvider, ORG: OrganizationBoundaryProvider> AuthorizerService<IAM, ORG> {
    /// Create a new instance of the authorizer service
    pub fn new(iam_provider: IAM, org_provider: ORG, policy_evaluator: PolicyEvaluator) -> Self {
        Self {
            iam_provider,
            org_provider,
            policy_evaluator,
        }
    }

    /// Check if a principal is authorized to perform an action on a resource
    pub async fn is_authorized(&self, request: AuthorizationRequest) -> Result<AuthorizationResponse, AuthorizationError> {
        // Get IAM policies for the principal
        let iam_policies = self.iam_provider.get_identity_policies_for(&request.principal).await?;
        
        // Get effective SCPs for the principal's account
        let effective_scps = self.org_provider.get_effective_scps_for(&request.principal).await?;
        
        // Combine IAM policies and SCPs
        let mut combined_policies = iam_policies;
        for scp in effective_scps {
            combined_policies.add_policy(scp.policy.clone());
        }
        
        // Evaluate the combined policies
        let response = self.policy_evaluator.evaluate(&combined_policies, &request);
        
        Ok(response)
    }
}
</file>

<file path="crates/hodei-authorizer/README.md">
# hodei-authorizer

El crate `hodei-authorizer` es el cerebro orquestador del sistema de gobernanza y autorización de Hodei. Combina políticas de IAM con Service Control Policies (SCPs) para tomar decisiones de acceso seguras y jerárquicas.

## Arquitectura

Este crate sigue una arquitectura limpia con separación clara de concerns:

- **Puertos (Ports)**: Traits abstractos que definen las interfaces necesarias
- **Adaptadores (Adapters)**: Implementaciones concretas de los puertos
- **Servicio de Autorización**: Lógica central que orquesta la evaluación de políticas

## Puertos

### IamPolicyProvider
```rust
#[async_trait]
pub trait IamPolicyProvider: Send + Sync {
    async fn get_identity_policies_for(&self, principal_hrn: &Hrn) -> Result<PolicySet, AuthorizationError>;
}
```

### OrganizationBoundaryProvider
```rust
#[async_trait]
pub trait OrganizationBoundaryProvider: Send + Sync {
    async fn get_effective_scps_for(&self, entity_hrn: &Hrn) -> Result<Vec<ServiceControlPolicy>, AuthorizationError>;
}
```

## Servicio de Autorización

### AuthorizerService
```rust
pub struct AuthorizerService<IAM: IamPolicyProvider, ORG: OrganizationBoundaryProvider> {
    iam_provider: IAM,
    org_provider: ORG,
    policy_evaluator: PolicyEvaluator,
}

impl<IAM: IamPolicyProvider, ORG: OrganizationBoundaryProvider> AuthorizerService<IAM, ORG> {
    pub fn new(iam_provider: IAM, org_provider: ORG, policy_evaluator: PolicyEvaluator) -> Self { ... }
    pub async fn is_authorized(&self, request: AuthorizationRequest) -> Result<AuthorizationResponse, AuthorizationError> { ... }
}
```

## Flujo de Autorización

1. El `AuthorizerService` recibe una solicitud de autorización
2. Obtiene las políticas de IAM del principal a través de `IamPolicyProvider`
3. Obtiene las SCPs efectivas para la entidad del principal a través de `OrganizationBoundaryProvider`
4. Combina todas las políticas y las evalúa usando el `PolicyEvaluator` de `hodei-policies`
5. Devuelve una respuesta de autorización con la decisión final

## Reglas de Autorización

1. **Deny Explícito Anula Todo**: Si cualquier política (IAM o SCP) contiene un `forbid` que coincida con la solicitud, la decisión final es `Deny`
2. **Se Requiere un Allow de Identidad**: Si no hay un `Deny` explícito, se requiere que las políticas de IAM contengan un `permit` explícito
3. **Las Barreras de la Organización Deben Permitir**: Si hay un `Allow` de IAM, las SCPs efectivas también deben permitir la acción

## Tests

Los tests están ubicados en el directorio `tests/` y utilizan mocks para probar la lógica de autorización sin depender de implementaciones concretas de los puertos.
</file>

<file path="crates/hodei-iam/src/features/add_user_to_group/mod.rs">
/// Feature: Add User to Group
///
/// This feature allows adding users to existing groups

pub mod dto;
pub mod use_case;
pub mod di;

pub use use_case::AddUserToGroupUseCase;
</file>

<file path="crates/hodei-iam/src/features/create_group/mod.rs">
/// Feature: Create Group
///
/// This feature allows creating new groups in the IAM system

pub mod dto;
pub mod use_case;
pub mod di;

pub use dto::{CreateGroupCommand, GroupView};
pub use use_case::CreateGroupUseCase;
</file>

<file path="crates/hodei-iam/src/features/create_user/mod.rs">
/// Feature: Create User
///
/// This feature allows creating new users in the IAM system

pub mod dto;
pub mod use_case;
pub mod di;

pub use use_case::CreateUserUseCase;
</file>

<file path="crates/hodei-iam/src/features/get_effective_policies_for_principal/adapter.rs">
//! Adaptadores para el caso de uso get_effective_policies_for_principal
//!
//! Estos adaptadores conectan los ports segregados del caso de uso con
//! los repositorios de la capa de aplicación compartida.

use crate::features::get_effective_policies_for_principal::ports::{
    GroupFinderPort, PolicyFinderPort, UserFinderPort,
};
use crate::shared::application::ports::{GroupRepository, UserRepository};
use crate::shared::domain::{Group, User};
use policies::shared::domain::hrn::Hrn;
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
        self.repository.find_by_hrn(hrn).await.map_err(|e| {
            // Convert anyhow error to a simple std::io::Error carrying the string message
            Box::new(std::io::Error::other(e.to_string()))
                as Box<dyn std::error::Error + Send + Sync>
        })
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
        // Marcar uso explícito del repositorio para evitar warning de campo no usado
        let _ = &self.repository;

        // TODO (pendiente): Implementar resolución real de grupos.
        Ok(vec![])
    }
}

/// Adaptador que conecta PolicyFinderPort con repositorio de políticas
///
/// NOTA: Actualmente no existe un PolicyRepository en hodei-iam.
/// Las políticas se gestionan en el crate 'policies'.
/// Este adaptador es un placeholder que devuelve políticas vacías.
///
/// En una implementación completa, necesitaríamos:
/// 1. Un PolicyRepository en hodei-iam que almacene la relación principal->policy
/// 2. O una integración con el crate 'policies' para buscar políticas por principal
pub struct PolicyFinderAdapter {
    // Placeholder - en el futuro inyectaríamos el repositorio real aquí
}

impl PolicyFinderAdapter {
    pub fn new() -> Self {
        Self {}
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
        // TODO: Implementar cuando tengamos PolicyRepository
        // Por ahora devolvemos un vector vacío
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::infrastructure::persistence::InMemoryUserRepository;

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
</file>

<file path="crates/hodei-iam/src/features/get_effective_policies_for_principal/di.rs">
//! Dependency Injection module for get_effective_policies_for_principal feature
//!
//! Provides factory functions to create instances of GetEffectivePoliciesForPrincipalUseCase
//! with the appropriate adapters and repositories wired together.

use super::adapter::{GroupFinderAdapter, PolicyFinderAdapter, UserFinderAdapter};
use super::use_case::GetEffectivePoliciesForPrincipalUseCase;
use crate::shared::application::ports::{GroupRepository, UserRepository};
use std::sync::Arc;

/// Create a GetEffectivePoliciesForPrincipalUseCase with in-memory repositories
///
/// This is the primary DI function for creating the use case in production.
/// It wires together all the necessary adapters and repositories.
///
/// # Arguments
/// * `user_repo` - User repository implementation
/// * `group_repo` - Group repository implementation
///
/// # Returns
/// Fully configured use case ready to execute
pub fn make_use_case<UR, GR>(
    user_repo: Arc<UR>,
    group_repo: Arc<GR>,
) -> GetEffectivePoliciesForPrincipalUseCase<
    UserFinderAdapter<UR>,
    GroupFinderAdapter<GR>,
    PolicyFinderAdapter,
>
where
    UR: UserRepository + 'static,
    GR: GroupRepository + 'static,
{
    let user_finder = Arc::new(UserFinderAdapter::new(user_repo));
    let group_finder = Arc::new(GroupFinderAdapter::new(group_repo));
    let policy_finder = Arc::new(PolicyFinderAdapter::new());

    GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::infrastructure::persistence::{InMemoryGroupRepository, InMemoryUserRepository};

    #[test]
    fn test_make_use_case() {
        let user_repo = Arc::new(InMemoryUserRepository::new());
        let group_repo = Arc::new(InMemoryGroupRepository::new());

        let _use_case = make_use_case(user_repo, group_repo);

        // If it compiles and constructs, the DI is working correctly
    }
}
</file>

<file path="crates/hodei-iam/src/features/get_effective_policies_for_principal/dto.rs">
use cedar_policy::PolicySet;
use serde::{Deserialize, Serialize};

/// Query to get effective IAM policies for a principal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEffectivePoliciesQuery {
    /// HRN of the principal (user, service account, etc.)
    pub principal_hrn: String,
}

/// Response containing effective IAM policies as a Cedar PolicySet
/// This is the PUBLIC interface - does not expose internal entities
#[derive(Debug, Clone)]
pub struct EffectivePoliciesResponse {
    /// Cedar PolicySet containing all effective IAM policies
    /// This includes:
    /// - Direct policies attached to the user
    /// - Policies from all groups the user belongs to
    /// - Policies from roles assigned to the user
    pub policies: PolicySet,
    /// HRN of the principal (for logging/debugging)
    pub principal_hrn: String,
    /// Number of policies included (for observability)
    pub policy_count: usize,
}

impl EffectivePoliciesResponse {
    pub fn new(policies: PolicySet, principal_hrn: String) -> Self {
        let policy_count = policies.policies().count();
        Self {
            policies,
            principal_hrn,
            policy_count,
        }
    }
}
</file>

<file path="crates/hodei-iam/src/features/get_effective_policies_for_principal/error.rs">
use thiserror::Error;

/// Errores específicos del caso de uso GetEffectivePoliciesForPrincipal
#[derive(Debug, Error)]
pub enum GetEffectivePoliciesError {
    #[error("Principal not found: {0}")]
    PrincipalNotFound(String),

    #[error("Invalid principal HRN: {0}")]
    InvalidPrincipalHrn(String),

    #[error("Invalid principal type: {0}. Expected 'user' or 'service-account'")]
    InvalidPrincipalType(String),

    #[error("Group not found: {0}")]
    GroupNotFound(String),

    #[error("Policy not found: {0}")]
    PolicyNotFound(String),

    #[error("Failed to parse policy document: {0}")]
    PolicyParseError(String),

    #[error("Repository error: {0}")]
    RepositoryError(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Tipo Result específico para este caso de uso
pub type GetEffectivePoliciesResult<T> = Result<T, GetEffectivePoliciesError>;
</file>

<file path="crates/hodei-iam/src/features/get_effective_policies_for_principal/ports.rs">
//! Ports for get_effective_policies_for_principal feature
//!
//! Define las interfaces (traits) que el caso de uso necesita para obtener
//! las políticas efectivas de un principal (usuario o service account).

use crate::shared::domain::{Group, User};
use policies::shared::domain::hrn::Hrn;

/// Port para encontrar usuarios por HRN
#[async_trait::async_trait]
pub trait UserFinderPort: Send + Sync {
    /// Find a user by their HRN
    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Port para encontrar grupos a los que pertenece un usuario
#[async_trait::async_trait]
pub trait GroupFinderPort: Send + Sync {
    /// Find all groups that a user belongs to
    async fn find_groups_by_user_hrn(
        &self,
        user_hrn: &Hrn,
    ) -> Result<Vec<Group>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Port para encontrar políticas asociadas a un principal
#[async_trait::async_trait]
pub trait PolicyFinderPort: Send + Sync {
    /// Find all policy documents associated with a principal (user or group)
    ///
    /// Returns policy documents in Cedar format as strings
    async fn find_policies_by_principal(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>>;
}
</file>

<file path="crates/hodei-iam/src/shared/application/mod.rs">
/// Application layer for hodei-iam

pub mod ports;
mod di_configurator;

pub use di_configurator::configure_default_iam_entities;
</file>

<file path="crates/hodei-iam/src/shared/domain/events.rs">
//! Domain events for the IAM bounded context
//!
//! These events represent state changes in the IAM domain that other
//! bounded contexts might be interested in.

use policies::domain::Hrn;
use serde::{Deserialize, Serialize};
use shared::application::ports::event_bus::DomainEvent;

/// Event emitted when a new user is created
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCreated {
    /// HRN of the created user
    pub user_hrn: Hrn,
    /// Username
    pub username: String,
    /// Email of the user
    pub email: String,
    /// Timestamp when the user was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for UserCreated {
    fn event_type(&self) -> &'static str {
        "iam.user.created"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.user_hrn.to_string())
    }
}

/// Event emitted when a user is updated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUpdated {
    /// HRN of the updated user
    pub user_hrn: Hrn,
    /// Username
    pub username: String,
    /// Timestamp when the user was updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for UserUpdated {
    fn event_type(&self) -> &'static str {
        "iam.user.updated"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.user_hrn.to_string())
    }
}

/// Event emitted when a user is deleted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDeleted {
    /// HRN of the deleted user
    pub user_hrn: Hrn,
    /// Timestamp when the user was deleted
    pub deleted_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for UserDeleted {
    fn event_type(&self) -> &'static str {
        "iam.user.deleted"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.user_hrn.to_string())
    }
}

/// Event emitted when a new group is created
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupCreated {
    /// HRN of the created group
    pub group_hrn: Hrn,
    /// Group name
    pub name: String,
    /// Timestamp when the group was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for GroupCreated {
    fn event_type(&self) -> &'static str {
        "iam.group.created"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}

/// Event emitted when a group is updated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupUpdated {
    /// HRN of the updated group
    pub group_hrn: Hrn,
    /// Group name
    pub name: String,
    /// Timestamp when the group was updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for GroupUpdated {
    fn event_type(&self) -> &'static str {
        "iam.group.updated"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}

/// Event emitted when a group is deleted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupDeleted {
    /// HRN of the deleted group
    pub group_hrn: Hrn,
    /// Timestamp when the group was deleted
    pub deleted_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for GroupDeleted {
    fn event_type(&self) -> &'static str {
        "iam.group.deleted"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}

/// Event emitted when a user is added to a group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAddedToGroup {
    /// HRN of the user
    pub user_hrn: Hrn,
    /// HRN of the group
    pub group_hrn: Hrn,
    /// Timestamp when the user was added
    pub added_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for UserAddedToGroup {
    fn event_type(&self) -> &'static str {
        "iam.user.added_to_group"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}

/// Event emitted when a user is removed from a group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRemovedFromGroup {
    /// HRN of the user
    pub user_hrn: Hrn,
    /// HRN of the group
    pub group_hrn: Hrn,
    /// Timestamp when the user was removed
    pub removed_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for UserRemovedFromGroup {
    fn event_type(&self) -> &'static str {
        "iam.user.removed_from_group"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}

/// Event emitted when a policy is attached to a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyAttachedToUser {
    /// HRN of the user
    pub user_hrn: Hrn,
    /// HRN of the policy
    pub policy_hrn: Hrn,
    /// Timestamp when the policy was attached
    pub attached_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for PolicyAttachedToUser {
    fn event_type(&self) -> &'static str {
        "iam.policy.attached_to_user"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.user_hrn.to_string())
    }
}

/// Event emitted when a policy is detached from a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDetachedFromUser {
    /// HRN of the user
    pub user_hrn: Hrn,
    /// HRN of the policy
    pub policy_hrn: Hrn,
    /// Timestamp when the policy was detached
    pub detached_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for PolicyDetachedFromUser {
    fn event_type(&self) -> &'static str {
        "iam.policy.detached_from_user"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.user_hrn.to_string())
    }
}

/// Event emitted when a policy is attached to a group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyAttachedToGroup {
    /// HRN of the group
    pub group_hrn: Hrn,
    /// HRN of the policy
    pub policy_hrn: Hrn,
    /// Timestamp when the policy was attached
    pub attached_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for PolicyAttachedToGroup {
    fn event_type(&self) -> &'static str {
        "iam.policy.attached_to_group"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}

/// Event emitted when a policy is detached from a group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDetachedFromGroup {
    /// HRN of the group
    pub group_hrn: Hrn,
    /// HRN of the policy
    pub policy_hrn: Hrn,
    /// Timestamp when the policy was detached
    pub detached_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for PolicyDetachedFromGroup {
    fn event_type(&self) -> &'static str {
        "iam.policy.detached_from_group"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.group_hrn.to_string())
    }
}
</file>

<file path="crates/hodei-iam/src/shared/infrastructure/surreal/group_repository.rs">
use crate::shared::application::ports::GroupRepository;
use crate::shared::domain::Group;
use policies::shared::domain::hrn::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::opt::RecordId;
use async_trait::async_trait;

/// SurrealDB implementation of GroupRepository
pub struct SurrealGroupRepository {
    db: Surreal<Any>,
}

impl SurrealGroupRepository {
    /// Create a new SurrealGroupRepository instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl GroupRepository for SurrealGroupRepository {
    async fn save(&self, group: &Group) -> Result<(), anyhow::Error> {
        let thing: RecordId = ("groups", group.hrn.to_string()).try_into()?;
        let _: surrealdb::opt::IntoRecordId = self.db.create(thing).content(group).await?;
        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, anyhow::Error> {
        let thing: RecordId = ("groups", hrn.to_string()).try_into()?;
        let group: Option<Group> = self.db.select(thing).await?;
        Ok(group)
    }

    async fn find_all(&self) -> Result<Vec<Group>, anyhow::Error> {
        let groups: Vec<Group> = self.db.select("groups").await?;
        Ok(groups)
    }
}
</file>

<file path="crates/hodei-iam/src/shared/infrastructure/surreal/iam_policy_provider.rs">
use crate::shared::infrastructure::surreal::{SurrealUserRepository, SurrealGroupRepository, policy_repository::IamPolicyRepository};
use crate::shared::application::ports::{UserRepository, GroupRepository};
use hodei_authorizer::features::evaluate_permissions::ports::{IamPolicyProvider, AuthorizationError};
use cedar_policy::PolicySet;
use policies::shared::domain::hrn::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use async_trait::async_trait;
use tracing::{info, warn, instrument};

/// SurrealDB implementation of IamPolicyProvider
pub struct SurrealIamPolicyProvider {
    user_repository: SurrealUserRepository,
    group_repository: SurrealGroupRepository,
    policy_repository: IamPolicyRepository,
}

impl SurrealIamPolicyProvider {
    /// Create a new SurrealIamPolicyProvider instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self {
            user_repository: SurrealUserRepository::new(db.clone()),
            group_repository: SurrealGroupRepository::new(db.clone()),
            policy_repository: IamPolicyRepository::new(db),
        }
    }
}

#[async_trait]
impl IamPolicyProvider for SurrealIamPolicyProvider {
    /// Get identity policies for a principal
    #[instrument(skip(self), fields(principal = %principal_hrn))]
    async fn get_identity_policies_for(&self, principal_hrn: &Hrn) -> Result<PolicySet, AuthorizationError> {
        info!("Retrieving IAM policies for principal: {}", principal_hrn);

        // Step 1: Get the user (principal)
        let user = self.user_repository.find_by_hrn(principal_hrn).await
            .map_err(|e| AuthorizationError::ProviderError(format!("Failed to find user {}: {}", principal_hrn, e)))?
            .ok_or_else(|| AuthorizationError::ProviderError(format!("User not found: {}", principal_hrn)))?;

        // Step 2: Collect all policy HRNs from user and groups
        let mut policy_hrns = Vec::new();

        // Add user's attached policies (if any - User entity doesn't have attached_policy_hrns in current implementation)
        // Note: This would require extending User entity to have attached_policy_hrns like Group

        // Add policies from all groups the user belongs to
        info!("User belongs to {} groups", user.group_hrns.len());
        for group_hrn in &user.group_hrns {
            match self.group_repository.find_by_hrn(group_hrn).await {
                Ok(Some(group)) => {
                    info!("Found group {} with {} attached policies", group_hrn, group.attached_policy_hrns.len());
                    policy_hrns.extend_from_slice(&group.attached_policy_hrns);
                }
                Ok(None) => {
                    warn!("Group {} not found for user {}", group_hrn, principal_hrn);
                }
                Err(e) => {
                    warn!("Failed to retrieve group {}: {}", group_hrn, e);
                }
            }
        }

        // Remove duplicates
        policy_hrns.sort();
        policy_hrns.dedup();
        info!("Found {} unique policy HRNs to retrieve", policy_hrns.len());

        // Step 3: Retrieve all policies and build PolicySet
        let mut policy_set = PolicySet::new();
        let mut successful_policies = 0;
        let mut failed_policies = 0;

        for policy_hrn in &policy_hrns {
            match self.policy_repository.find_by_hrn(policy_hrn).await {
                Ok(Some(iam_policy)) => {
                    match iam_policy.as_cedar_policy() {
                        Ok(cedar_policy) => {
                            policy_set.add_policy(cedar_policy);
                            successful_policies += 1;
                            info!("Successfully loaded policy: {}", policy_hrn);
                        }
                        Err(e) => {
                            failed_policies += 1;
                            warn!("Failed to parse policy {} as Cedar policy: {}", policy_hrn, e);
                        }
                    }
                }
                Ok(None) => {
                    failed_policies += 1;
                    warn!("Policy not found: {}", policy_hrn);
                }
                Err(e) => {
                    failed_policies += 1;
                    warn!("Failed to retrieve policy {}: {}", policy_hrn, e);
                }
            }
        }

        info!("Successfully loaded {} policies, {} failed", successful_policies, failed_policies);
        
        if failed_policies > 0 {
            warn!("Some policies failed to load for principal {}", principal_hrn);
        }

        Ok(policy_set)
    }
}
</file>

<file path="crates/hodei-iam/src/shared/infrastructure/surreal/mod.rs">
pub mod iam_policy_provider;
pub mod user_repository;
pub mod group_repository;
pub mod policy_repository;

pub use user_repository::SurrealUserRepository;
pub use group_repository::SurrealGroupRepository;
pub use policy_repository::IamPolicyRepository;
</file>

<file path="crates/hodei-iam/src/shared/infrastructure/surreal/policy_repository.rs">
use policies::shared::domain::hrn::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::opt::RecordId;
use async_trait::async_trait;
use cedar_policy::Policy;
use std::str::FromStr;

/// Policy entity for IAM
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IamPolicy {
    pub hrn: Hrn,
    pub name: String,
    pub policy_text: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

impl IamPolicy {
    /// Create a new IAM policy
    pub fn new(hrn: Hrn, name: String, policy_text: String) -> Self {
        Self {
            hrn,
            name,
            policy_text,
            description: None,
            tags: Vec::new(),
        }
    }

    /// Parse the policy text into a Cedar Policy
    pub fn as_cedar_policy(&self) -> Result<Policy, cedar_policy::ParseErrors> {
        Policy::from_str(&self.policy_text)
    }
}

/// Repository for IAM policies
pub struct IamPolicyRepository {
    db: Surreal<Any>,
}

impl IamPolicyRepository {
    /// Create a new IamPolicyRepository instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl IamPolicyRepository {
    /// Save a policy
    pub async fn save(&self, policy: &IamPolicy) -> Result<(), anyhow::Error> {
        let thing: RecordId = ("iam_policies", policy.hrn.to_string()).try_into()?;
        let _: surrealdb::opt::IntoRecordId = self.db.create(thing).content(policy).await?;
        Ok(())
    }

    /// Find policy by HRN
    pub async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<IamPolicy>, anyhow::Error> {
        let thing: RecordId = ("iam_policies", hrn.to_string()).try_into()?;
        let policy: Option<IamPolicy> = self.db.select(thing).await?;
        Ok(policy)
    }

    /// Find all policies
    pub async fn find_all(&self) -> Result<Vec<IamPolicy>, anyhow::Error> {
        let policies: Vec<IamPolicy> = self.db.select("iam_policies").await?;
        Ok(policies)
    }

    /// Find policies by HRNs
    pub async fn find_by_hrns(&self, hrns: &[Hrn]) -> Result<Vec<IamPolicy>, anyhow::Error> {
        let mut policies = Vec::new();
        for hrn in hrns {
            if let Some(policy) = self.find_by_hrn(hrn).await? {
                policies.push(policy);
            }
        }
        Ok(policies)
    }

    /// Delete a policy
    pub async fn delete(&self, hrn: &Hrn) -> Result<bool, anyhow::Error> {
        let thing: RecordId = ("iam_policies", hrn.to_string()).try_into()?;
        let result: Option<IamPolicy> = self.db.delete(thing).await?;
        Ok(result.is_some())
    }
}
</file>

<file path="crates/hodei-iam/src/shared/infrastructure/surreal/user_repository.rs">
use crate::shared::application::ports::UserRepository;
use crate::shared::domain::User;
use policies::shared::domain::hrn::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::opt::RecordId;
use async_trait::async_trait;

/// SurrealDB implementation of UserRepository
pub struct SurrealUserRepository {
    db: Surreal<Any>,
}

impl SurrealUserRepository {
    /// Create a new SurrealUserRepository instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl UserRepository for SurrealUserRepository {
    async fn save(&self, user: &User) -> Result<(), anyhow::Error> {
        let thing: RecordId = ("users", user.hrn.to_string()).try_into()?;
        let _: surrealdb::opt::IntoRecordId = self.db.create(thing).content(user).await?;
        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, anyhow::Error> {
        let thing: RecordId = ("users", hrn.to_string()).try_into()?;
        let user: Option<User> = self.db.select(thing).await?;
        Ok(user)
    }

    async fn find_all(&self) -> Result<Vec<User>, anyhow::Error> {
        let users: Vec<User> = self.db.select("users").await?;
        Ok(users)
    }
}
</file>

<file path="crates/hodei-iam/src/shared/infrastructure/mod.rs">
/// Infrastructure layer for hodei-iam
///
/// This module contains the adapters and implementations for infrastructure concerns
/// like persistence, external services, etc.

pub mod persistence;
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/adapter.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::shared::application::ports::scp_repository::{ScpRepository, ScpRepositoryError};
use crate::shared::application::ports::account_repository::{AccountRepository, AccountRepositoryError};
use crate::shared::application::ports::ou_repository::{OuRepository, OuRepositoryError};
use crate::features::attach_scp::ports::{ScpRepositoryPort, AccountRepositoryPort, OuRepositoryPort};
use policies::domain::Hrn;
use async_trait::async_trait;

/// Adapter that implements the ScpRepositoryPort trait using the ScpRepository
pub struct ScpRepositoryAdapter<SR: ScpRepository + std::marker::Send> {
    repository: SR,
}

impl<SR: ScpRepository + std::marker::Send> ScpRepositoryAdapter<SR> {
    /// Create a new adapter instance
    pub fn new(repository: SR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<SR: ScpRepository + std::marker::Sync + std::marker::Send> ScpRepositoryPort for ScpRepositoryAdapter<SR> {
    /// Find an SCP by HRN
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
}

/// Adapter that implements the AccountRepositoryPort trait using the AccountRepository
pub struct AccountRepositoryAdapter<AR: AccountRepository + std::marker::Send> {
    repository: AR,
}

impl<AR: AccountRepository + std::marker::Send> AccountRepositoryAdapter<AR> {
    /// Create a new adapter instance
    pub fn new(repository: AR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<AR: AccountRepository + std::marker::Sync + std::marker::Send> AccountRepositoryPort for AccountRepositoryAdapter<AR> {
    /// Find an account by HRN
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
    
    /// Save an account
    async fn save_account(&self, account: Account) -> Result<(), AccountRepositoryError> {
        self.repository.save(&account).await
    }
}

/// Adapter that implements the OuRepositoryPort trait using the OuRepository
pub struct OuRepositoryAdapter<OR: OuRepository + std::marker::Send> {
    repository: OR,
}

impl<OR: OuRepository + std::marker::Send> OuRepositoryAdapter<OR> {
    /// Create a new adapter instance
    pub fn new(repository: OR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<OR: OuRepository + std::marker::Sync + std::marker::Send> OuRepositoryPort for OuRepositoryAdapter<OR> {
    /// Find an OU by HRN
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
    
    /// Save an OU
    async fn save_ou(&self, ou: OrganizationalUnit) -> Result<(), OuRepositoryError> {
        self.repository.save(&ou).await
    }
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/dto.rs">
use serde::{Deserialize, Serialize};

/// Command to attach an SCP to an entity (Account or OU)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachScpCommand {
    /// HRN of the SCP to attach
    pub scp_hrn: String,
    /// HRN of the target entity (Account or OU)
    pub target_hrn: String,
}

/// View of the attach SCP operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachScpView {
    /// HRN of the SCP that was attached
    pub scp_hrn: String,
    /// HRN of the target entity
    pub target_hrn: String,
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/error.rs">
use thiserror::Error;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;

/// Error type for attach SCP use case
#[derive(Debug, Error)]
pub enum AttachScpError {
    #[error("SCP repository error: {0}")]
    ScpRepository(#[from] ScpRepositoryError),
    #[error("Account repository error: {0}")]
    AccountRepository(#[from] AccountRepositoryError),
    #[error("OU repository error: {0}")]
    OuRepository(#[from] OuRepositoryError),
    #[error("SCP not found: {0}")]
    ScpNotFound(String),
    #[error("Target entity not found: {0}")]
    TargetNotFound(String),
    #[error("Invalid target entity type: {0}")]
    InvalidTargetType(String),
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/mocks.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;
use crate::features::attach_scp::ports::{ScpRepositoryPort, AccountRepositoryPort, OuRepositoryPort};
use policies::domain::Hrn;

use std::collections::HashMap;
use std::sync::RwLock;
use async_trait::async_trait;

/// Mock implementation of ScpRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockScpRepositoryPort {
    scps: RwLock<HashMap<String, ServiceControlPolicy>>,
}

impl MockScpRepositoryPort {
    pub fn new() -> Self {
        Self {
            scps: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_scp(self, scp: ServiceControlPolicy) -> Self {
        let hrn_string = scp.hrn.to_string();
        self.scps.write().unwrap().insert(hrn_string, scp);
        self
    }
}

#[async_trait]
impl ScpRepositoryPort for MockScpRepositoryPort {
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let scps = self.scps.read().unwrap();
        Ok(scps.get(&hrn.to_string()).cloned())
    }
}

/// Mock implementation of AccountRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockAccountRepositoryPort {
    accounts: RwLock<HashMap<String, Account>>,
}

impl MockAccountRepositoryPort {
    pub fn new() -> Self {
        Self {
            accounts: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_account(self, account: Account) -> Self {
        let hrn_string = account.hrn.to_string();
        self.accounts.write().unwrap().insert(hrn_string, account);
        self
    }

    pub fn update_account(&self, account: Account) {
        let hrn_string = account.hrn.to_string();
        self.accounts.write().unwrap().insert(hrn_string, account);
    }
}

#[async_trait]
impl AccountRepositoryPort for MockAccountRepositoryPort {
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        let accounts = self.accounts.read().unwrap();
        Ok(accounts.get(&hrn.to_string()).cloned())
    }

    async fn save_account(&self, account: Account) -> Result<(), AccountRepositoryError> {
        let mut accounts = self.accounts.write().unwrap();
        accounts.insert(account.hrn.to_string(), account);
        Ok(())
    }
}

/// Mock implementation of OuRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockOuRepositoryPort {
    ous: RwLock<HashMap<String, OrganizationalUnit>>,
}

impl MockOuRepositoryPort {
    pub fn new() -> Self {
        Self {
            ous: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_ou(self, ou: OrganizationalUnit) -> Self {
        let hrn_string = ou.hrn.to_string();
        self.ous.write().unwrap().insert(hrn_string, ou);
        self
    }

    pub fn update_ou(&self, ou: OrganizationalUnit) {
        let hrn_string = ou.hrn.to_string();
        self.ous.write().unwrap().insert(hrn_string, ou);
    }
}

#[async_trait]
impl OuRepositoryPort for MockOuRepositoryPort {
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        let ous = self.ous.read().unwrap();
        Ok(ous.get(&hrn.to_string()).cloned())
    }

    async fn save_ou(&self, ou: OrganizationalUnit) -> Result<(), OuRepositoryError> {
        let mut ous = self.ous.write().unwrap();
        ous.insert(ou.hrn.to_string(), ou);
        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/mod.rs">
pub mod use_case;
pub mod dto;
pub mod error;
pub mod ports;
pub mod adapter;
pub mod di;
pub mod mocks;
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/ports.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;
use policies::domain::Hrn;

/// Port for retrieving service control policies
#[async_trait::async_trait]
pub trait ScpRepositoryPort: Send + Sync {
    /// Find an SCP by HRN
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError>;
}

/// Port for retrieving and updating accounts
#[async_trait::async_trait]
pub trait AccountRepositoryPort: Send + Sync {
    /// Find an account by HRN
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError>;
    
    /// Save an account
    async fn save_account(&self, account: Account) -> Result<(), AccountRepositoryError>;
}

/// Port for retrieving and updating organizational units
#[async_trait::async_trait]
pub trait OuRepositoryPort: Send + Sync {
    /// Find an OU by HRN
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError>;
    
    /// Save an OU
    async fn save_ou(&self, ou: OrganizationalUnit) -> Result<(), OuRepositoryError>;
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/use_case_test.rs">
use crate::features::attach_scp::dto::{AttachScpCommand, AttachScpView};
use crate::features::attach_scp::use_case::AttachScpUseCase;
use crate::features::attach_scp::mocks::{MockScpRepositoryPort, MockAccountRepositoryPort, MockOuRepositoryPort};
use crate::shared::domain::{ServiceControlPolicy, Account, OrganizationalUnit};
use policies::shared::domain::hrn::Hrn;

#[tokio::test]
async fn test_attach_scp_to_account() {
    // Arrange
    let scp_repository = MockScpRepositoryPort::new();
    let account_repository = MockAccountRepositoryPort::new();
    let ou_repository = MockOuRepositoryPort::new();
    
    // Create test entities
    let scp_hrn = Hrn::new("scp", "test-scp");
    let account_hrn = Hrn::new("account", "test-account");
    let parent_ou_hrn = Hrn::new("ou", "parent-ou");
    
    let scp = ServiceControlPolicy::new(
        scp_hrn.clone(),
        "TestSCP".to_string(),
        "permit(principal, action, resource);".to_string(),
    );
    
    let account = Account::new(
        account_hrn.clone(),
        "TestAccount".to_string(),
        parent_ou_hrn.clone(),
    );
    
    // Populate mocks
    scp_repository.with_scp(scp);
    account_repository.with_account(account);
    
    // Create use case
    let use_case = AttachScpUseCase::new(scp_repository, account_repository, ou_repository);
    
    // Create command
    let command = AttachScpCommand {
        scp_hrn: scp_hrn.to_string(),
        target_hrn: account_hrn.to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let attach_view = result.unwrap();
    assert_eq!(attach_view.scp_hrn, scp_hrn.to_string());
    assert_eq!(attach_view.target_hrn, account_hrn.to_string());
}

#[tokio::test]
async fn test_attach_scp_to_ou() {
    // Arrange
    let scp_repository = MockScpRepositoryPort::new();
    let account_repository = MockAccountRepositoryPort::new();
    let ou_repository = MockOuRepositoryPort::new();
    
    // Create test entities
    let scp_hrn = Hrn::new("scp", "test-scp");
    let ou_hrn = Hrn::new("ou", "test-ou");
    let parent_ou_hrn = Hrn::new("ou", "parent-ou");
    
    let scp = ServiceControlPolicy::new(
        scp_hrn.clone(),
        "TestSCP".to_string(),
        "permit(principal, action, resource);".to_string(),
    );
    
    let ou = OrganizationalUnit::new(
        ou_hrn.clone(),
        "TestOU".to_string(),
        parent_ou_hrn.clone(),
    );
    
    // Populate mocks
    scp_repository.with_scp(scp);
    ou_repository.with_ou(ou);
    
    // Create use case
    let use_case = AttachScpUseCase::new(scp_repository, account_repository, ou_repository);
    
    // Create command
    let command = AttachScpCommand {
        scp_hrn: scp_hrn.to_string(),
        target_hrn: ou_hrn.to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let attach_view = result.unwrap();
    assert_eq!(attach_view.scp_hrn, scp_hrn.to_string());
    assert_eq!(attach_view.target_hrn, ou_hrn.to_string());
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/error.rs">
use thiserror::Error;
use crate::shared::application::ports::account_repository::AccountRepositoryError;

#[derive(Debug, Error)]
pub enum CreateAccountError {
    #[error("Account repository error: {0}")]
    AccountRepositoryError(#[from] AccountRepositoryError),
    #[error("Invalid account name")]
    InvalidAccountName,
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/mocks.rs">
use crate::features::create_account::ports::AccountPersister;
use crate::features::create_account::error::CreateAccountError;
use crate::shared::domain::account::Account;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::Arc;

pub struct MockAccountPersister {
    accounts: Arc<Mutex<HashMap<String, Account>>>,
}

impl MockAccountPersister {
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl AccountPersister for MockAccountPersister {
    async fn save(&self, account: Account) -> Result<(), CreateAccountError> {
        let mut accounts = self.accounts.lock().unwrap();
        accounts.insert(account.hrn.to_string(), account);
        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/ports.rs">
use crate::shared::domain::account::Account;
use crate::features::create_account::error::CreateAccountError;
use async_trait::async_trait;

#[async_trait]
pub trait AccountPersister {
    async fn save(&self, account: Account) -> Result<(), CreateAccountError>;
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/adapter.rs">
use crate::features::create_ou::ports::OuPersister;
use crate::features::create_ou::error::CreateOuError;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::shared::application::ports::ou_repository::OuRepository;
use async_trait::async_trait;
use std::sync::Arc;

pub struct OuPersisterAdapter<OR: OuRepository> {
    repository: Arc<OR>,
}

impl<OR: OuRepository> OuPersisterAdapter<OR> {
    pub fn new(repository: Arc<OR>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<OR: OuRepository> OuPersister for OuPersisterAdapter<OR> {
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), CreateOuError> {
        self.repository.save(&ou).await?;
        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/di.rs">
use crate::shared::application::ports::OuRepository;
use crate::features::create_ou::use_case::CreateOuUseCase;
use crate::features::create_ou::adapter::OuRepositoryAdapter;

/// Create an instance of the CreateOuUseCase with the provided repository
pub fn create_ou_use_case<OR: OuRepository>(
    ou_repository: OR,
) -> CreateOuUseCase<OuRepositoryAdapter<OR>> {
    let adapter = OuRepositoryAdapter::new(ou_repository);
    CreateOuUseCase::new(adapter)
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/dto.rs">
use serde::{Deserialize, Serialize};
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOuCommand {
    pub name: String,
    pub parent_hrn: Hrn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OuView {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/error.rs">
use thiserror::Error;
use crate::shared::application::ports::ou_repository::OuRepositoryError;

#[derive(Debug, Error)]
pub enum CreateOuError {
    #[error("OU repository error: {0}")]
    OuRepositoryError(#[from] OuRepositoryError),
    #[error("Invalid OU name")]
    InvalidOuName,
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/mocks.rs">
use crate::features::create_ou::ports::OuPersister;
use crate::features::create_ou::error::CreateOuError;
use crate::shared::domain::ou::OrganizationalUnit;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::Arc;

pub struct MockOuPersister {
    ous: Arc<Mutex<HashMap<String, OrganizationalUnit>>>,
}

impl MockOuPersister {
    pub fn new() -> Self {
        Self {
            ous: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl OuPersister for MockOuPersister {
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), CreateOuError> {
        let mut ous = self.ous.lock().unwrap();
        ous.insert(ou.hrn.to_string(), ou);
        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/mod.rs">
pub mod use_case;
pub mod ports;
pub mod error;
pub mod dto;
#[cfg(test)]
pub mod use_case_test;
#[cfg(test)]
pub mod mocks;
</file>

<file path="crates/hodei-organizations/src/features/create_ou/ports.rs">
use crate::shared::domain::ou::OrganizationalUnit;
use crate::features::create_ou::error::CreateOuError;
use async_trait::async_trait;

#[async_trait]
pub trait OuPersister {
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), CreateOuError>;
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/use_case_test.rs">
use crate::features::create_ou::use_case::CreateOuUseCase;
use crate::features::create_ou::dto::{CreateOuCommand};
use crate::features::create_ou::error::CreateOuError;
use crate::features::create_ou::mocks::MockOuPersister;

use std::sync::Arc;
use policies::domain::Hrn;

#[tokio::test]
async fn test_create_ou_success() {
    // Arrange
    let mock_persister = MockOuPersister::new();
    let use_case = CreateOuUseCase::new(Arc::new(mock_persister));
    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "root".to_string(),
        "r-123".to_string(),
    );
    let command = CreateOuCommand {
        name: "TestOU".to_string(),
        parent_hrn: parent_hrn.clone(),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok());
    let ou_view = result.unwrap();
    assert_eq!(ou_view.name, "TestOU");
    assert_eq!(ou_view.parent_hrn, parent_hrn);
    assert!(!ou_view.hrn.to_string().is_empty());
}

#[tokio::test]
async fn test_create_ou_empty_name() {
    // Arrange
    let mock_persister = MockOuPersister::new();
    let use_case = CreateOuUseCase::new(Arc::new(mock_persister));
    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "root".to_string(),
        "r-123".to_string(),
    );
    let command = CreateOuCommand {
        name: "".to_string(),
        parent_hrn,
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(error, CreateOuError::InvalidOuName));
}
</file>

<file path="crates/hodei-organizations/src/features/create_ou/use_case.rs">
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
</file>

<file path="crates/hodei-organizations/src/features/create_scp/adapter.rs">
use crate::shared::domain::ServiceControlPolicy;
use crate::shared::application::ports::{ScpRepository, ScpRepositoryError};
use crate::features::create_scp::ports::ScpPersister;
use async_trait::async_trait;

/// Adapter that implements the ScpPersister trait using the ScpRepository
pub struct ScpRepositoryAdapter<SR: ScpRepository> {
    repository: SR,
}

impl<SR: ScpRepository> ScpRepositoryAdapter<SR> {
    /// Create a new adapter instance
    pub fn new(repository: SR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<SR: ScpRepository> ScpPersister for ScpRepositoryAdapter<SR> {
    /// Save an SCP using the repository
    async fn save(&self, scp: ServiceControlPolicy) -> Result<(), ScpRepositoryError> {
        self.repository.save(&scp).await
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/di.rs">
use crate::shared::application::ports::ScpRepository;
use crate::features::create_scp::use_case::CreateScpUseCase;
use crate::features::create_scp::adapter::ScpRepositoryAdapter;

/// Create an instance of the CreateScpUseCase with the provided repository
pub fn create_scp_use_case<SR: ScpRepository>(
    scp_repository: SR,
) -> CreateScpUseCase<ScpRepositoryAdapter<SR>> {
    let adapter = ScpRepositoryAdapter::new(scp_repository);
    CreateScpUseCase::new(adapter)
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/dto.rs">
use serde::{Deserialize, Serialize};
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScpCommand {
    pub name: String,
    pub document: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScpView {
    pub hrn: Hrn,
    pub name: String,
    pub document: String,
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/error.rs">
use thiserror::Error;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;

#[derive(Debug, Error)]
pub enum CreateScpError {
    #[error("SCP repository error: {0}")]
    ScpRepositoryError(#[from] ScpRepositoryError),
    #[error("Invalid SCP name")]
    InvalidScpName,
    #[error("Invalid SCP document")]
    InvalidScpDocument,
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/mocks.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use crate::shared::application::ports::scp_repository::{ScpRepository, ScpRepositoryError};
use crate::features::create_scp::ports::ScpPersister;
use crate::features::create_scp::error::CreateScpError;
use policies::shared::domain::hrn::Hrn;
use std::collections::HashMap;
use std::sync::RwLock;
use async_trait::async_trait;

/// Mock implementation of ScpRepository for testing
#[derive(Debug, Default)]
pub struct MockScpRepository {
    scps: RwLock<HashMap<String, ServiceControlPolicy>>,
}

impl MockScpRepository {
    pub fn new() -> Self {
        Self {
            scps: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_scp(self, scp: ServiceControlPolicy) -> Self {
        let hrn_string = scp.hrn.to_string();
        self.scps.write().unwrap().insert(hrn_string, scp);
        self
    }
}

#[async_trait]
impl ScpRepository for MockScpRepository {
    async fn save(&self, scp: &ServiceControlPolicy) -> Result<(), ScpRepositoryError> {
        let mut scps = self.scps.write().unwrap();
        scps.insert(scp.hrn.to_string(), scp.clone());
        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let scps = self.scps.read().unwrap();
        Ok(scps.get(&hrn.to_string()).cloned())
    }
}

/// Mock implementation of ScpPersister for testing
#[derive(Debug, Default)]
pub struct MockScpPersister {
    saved_scps: RwLock<Vec<ServiceControlPolicy>>,
}

impl MockScpPersister {
    pub fn new() -> Self {
        Self {
            saved_scps: RwLock::new(Vec::new()),
        }
    }

    pub fn get_saved_scps(&self) -> Vec<ServiceControlPolicy> {
        self.saved_scps.read().unwrap().clone()
    }
}

#[async_trait]
impl ScpPersister for MockScpPersister {
    async fn save(&self, scp: ServiceControlPolicy) -> Result<(), CreateScpError> {
        let mut saved_scps = self.saved_scps.write().unwrap();
        saved_scps.push(scp);
        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/mod.rs">
pub mod use_case;
pub mod ports;
pub mod error;
pub mod dto;
#[cfg(test)]
pub mod use_case_test;
#[cfg(test)]
pub mod mocks;
</file>

<file path="crates/hodei-organizations/src/features/create_scp/ports.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use crate::features::create_scp::error::CreateScpError;
use async_trait::async_trait;

#[async_trait]
pub trait ScpPersister {
    async fn save(&self, scp: ServiceControlPolicy) -> Result<(), CreateScpError>;
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/use_case_test.rs">
use crate::features::create_scp::dto::CreateScpCommand;
use crate::features::create_scp::use_case::CreateScpUseCase;
use crate::features::create_scp::mocks::MockScpPersister;
use crate::features::create_scp::dto::ScpView;

#[tokio::test]
async fn test_create_scp_use_case() {
    // Arrange
    let persister = MockScpPersister::new();
    let use_case = CreateScpUseCase::new(std::sync::Arc::new(persister));
    let command = CreateScpCommand {
        name: "TestSCP".to_string(),
        document: "permit(principal, action, resource);".to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let scp_view = result.unwrap();
    assert_eq!(scp_view.name, "TestSCP");
    assert_eq!(scp_view.document, "permit(principal, action, resource);");
    assert!(scp_view.hrn.to_string().starts_with("hrn:"));
}
</file>

<file path="crates/hodei-organizations/src/features/create_scp/use_case.rs">
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
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/adapter.rs">
use crate::shared::domain::{ServiceControlPolicy, Account, OrganizationalUnit};
use crate::shared::application::ports::scp_repository::{ScpRepository, ScpRepositoryError};
use crate::shared::application::ports::account_repository::{AccountRepository, AccountRepositoryError};
use crate::shared::application::ports::ou_repository::{OuRepository, OuRepositoryError};
use crate::features::get_effective_scps::ports::{ScpRepositoryPort, AccountRepositoryPort, OuRepositoryPort};
use policies::shared::domain::hrn::Hrn;
use async_trait::async_trait;

/// Adapter that implements the ScpRepositoryPort trait using the ScpRepository
pub struct ScpRepositoryAdapter<SR: ScpRepository> {
    repository: SR,
}

impl<SR: ScpRepository> ScpRepositoryAdapter<SR> {
    /// Create a new adapter instance
    pub fn new(repository: SR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<SR: ScpRepository> ScpRepositoryPort for ScpRepositoryAdapter<SR> {
    /// Find an SCP by HRN
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
}

/// Adapter that implements the AccountRepositoryPort trait using the AccountRepository
pub struct AccountRepositoryAdapter<AR: AccountRepository + Send + Sync> {
    repository: AR,
}

impl<AR: AccountRepository + Send + Sync> AccountRepositoryAdapter<AR> {
    /// Create a new adapter instance
    pub fn new(repository: AR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<AR: AccountRepository + Send + Sync> AccountRepositoryPort for AccountRepositoryAdapter<AR> {
    /// Find an account by HRN
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
}

/// Adapter that implements the OuRepositoryPort trait using the OuRepository
pub struct OuRepositoryAdapter<OR: OuRepository + Send + Sync> {
    repository: OR,
}

impl<OR: OuRepository + Send + Sync> OuRepositoryAdapter<OR> {
    /// Create a new adapter instance
    pub fn new(repository: OR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<OR: OuRepository + Send + Sync> OuRepositoryPort for OuRepositoryAdapter<OR> {
    /// Find an OU by HRN
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        self.repository.find_by_hrn(hrn).await
    }
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/di.rs">
use crate::shared::application::ports::scp_repository::ScpRepository;
use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::application::ports::ou_repository::OuRepository;
use crate::features::get_effective_scps::use_case::GetEffectiveScpsUseCase;
use crate::features::get_effective_scps::adapter::{
    ScpRepositoryAdapter,
    AccountRepositoryAdapter,
    OuRepositoryAdapter,
};

/// Adaptador combinado que expone tanto cuentas como OUs
pub struct OrgRepositoryAdapter<AR, OR>
where
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    account_adapter: AccountRepositoryAdapter<AR>,
    ou_adapter: OuRepositoryAdapter<OR>,
}

impl<AR, OR> OrgRepositoryAdapter<AR, OR>
where
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    pub fn new(account_repo: AR, ou_repo: OR) -> Self {
        Self {
            account_adapter: AccountRepositoryAdapter::new(account_repo),
            ou_adapter: OuRepositoryAdapter::new(ou_repo),
        }
    }
}

#[async_trait::async_trait]
impl<AR, OR> crate::features::get_effective_scps::ports::AccountRepositoryPort for OrgRepositoryAdapter<AR, OR>
where
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    async fn find_account_by_hrn(&self, hrn: &policies::shared::domain::hrn::Hrn) -> Result<Option<crate::shared::domain::Account>, crate::shared::application::ports::account_repository::AccountRepositoryError> {
        self.account_adapter.find_account_by_hrn(hrn).await
    }
}

#[async_trait::async_trait]
impl<AR, OR> crate::features::get_effective_scps::ports::OuRepositoryPort for OrgRepositoryAdapter<AR, OR>
where
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    async fn find_ou_by_hrn(&self, hrn: &policies::shared::domain::hrn::Hrn) -> Result<Option<crate::shared::domain::OrganizationalUnit>, crate::shared::application::ports::ou_repository::OuRepositoryError> {
        self.ou_adapter.find_ou_by_hrn(hrn).await
    }
}

/// Crea el caso de uso con repositorios concretos Surreal u otros
pub fn get_effective_scps_use_case<SR, AR, OR>(
    scp_repository: SR,
    account_repository: AR,
    ou_repository: OR,
) -> GetEffectiveScpsUseCase<ScpRepositoryAdapter<SR>, OrgRepositoryAdapter<AR, OR>>
where
    SR: ScpRepository + Send + Sync,
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    let scp_adapter = ScpRepositoryAdapter::new(scp_repository);
    let org_adapter = OrgRepositoryAdapter::new(account_repository, ou_repository);
    GetEffectiveScpsUseCase::new(scp_adapter, org_adapter)
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/error.rs">
use thiserror::Error;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;

/// Error type for get effective SCPs use case
#[derive(Debug, Error)]
pub enum GetEffectiveScpsError {
    #[error("SCP repository error: {0}")]
    ScpRepository(#[from] ScpRepositoryError),
    #[error("Account repository error: {0}")]
    AccountRepository(#[from] AccountRepositoryError),
    #[error("OU repository error: {0}")]
    OuRepository(#[from] OuRepositoryError),
    #[error("Target entity not found: {0}")]
    TargetNotFound(String),
    #[error("Invalid target entity type: {0}")]
    InvalidTargetType(String),
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/mocks.rs">
use crate::features::get_effective_scps::ports::{
    AccountRepositoryPort, OuRepositoryPort, ScpRepositoryPort,
};
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::domain::{Account, OrganizationalUnit, ServiceControlPolicy};
use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;
use std::collections::HashMap;
use std::sync::RwLock;

/// Mock implementation of ScpRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockScpRepositoryPort {
    scps: RwLock<HashMap<String, ServiceControlPolicy>>,
}

impl MockScpRepositoryPort {
    pub fn new() -> Self {
        Self {
            scps: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_scp(self, scp: ServiceControlPolicy) -> Self {
        let hrn_string = scp.hrn.to_string();
        self.scps.write().unwrap().insert(hrn_string, scp);
        self
    }
}

#[async_trait]
impl ScpRepositoryPort for MockScpRepositoryPort {
    async fn find_scp_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let scps = self.scps.read().unwrap();
        Ok(scps.get(&hrn.to_string()).cloned())
    }
}

/// Mock implementation of AccountRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockAccountRepositoryPort {
    accounts: RwLock<HashMap<String, Account>>,
}

impl MockAccountRepositoryPort {
    pub fn new() -> Self {
        Self {
            accounts: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_account(self, account: Account) -> Self {
        let hrn_string = account.hrn.to_string();
        self.accounts.write().unwrap().insert(hrn_string, account);
        self
    }
}

#[async_trait]
impl AccountRepositoryPort for MockAccountRepositoryPort {
    async fn find_account_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<Account>, AccountRepositoryError> {
        let accounts = self.accounts.read().unwrap();
        Ok(accounts.get(&hrn.to_string()).cloned())
    }
}

/// Mock implementation of OuRepositoryPort for testing
#[derive(Debug, Default)]
pub struct MockOuRepositoryPort {
    ous: RwLock<HashMap<String, OrganizationalUnit>>,
}

impl MockOuRepositoryPort {
    pub fn new() -> Self {
        Self {
            ous: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_ou(self, ou: OrganizationalUnit) -> Self {
        let hrn_string = ou.hrn.to_string();
        self.ous.write().unwrap().insert(hrn_string, ou);
        self
    }
}

#[async_trait]
impl OuRepositoryPort for MockOuRepositoryPort {
    async fn find_ou_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        let ous = self.ous.read().unwrap();
        Ok(ous.get(&hrn.to_string()).cloned())
    }
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/ports.rs">
use crate::shared::domain::{ServiceControlPolicy, Account, OrganizationalUnit};
use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;
use policies::shared::domain::hrn::Hrn;

/// Port for retrieving service control policies
#[async_trait::async_trait]
pub trait ScpRepositoryPort: Send + Sync {
    /// Find an SCP by HRN
    async fn find_scp_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError>;
}

/// Port for retrieving accounts
#[async_trait::async_trait]
pub trait AccountRepositoryPort: Send + Sync {
    /// Find an account by HRN
    async fn find_account_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError>;
}

/// Port for retrieving organizational units
#[async_trait::async_trait]
pub trait OuRepositoryPort: Send + Sync {
    /// Find an OU by HRN
    async fn find_ou_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError>;
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/use_case_test.rs">
use crate::features::get_effective_scps::dto::{GetEffectiveScpsCommand, EffectiveScpsView};
use crate::features::get_effective_scps::use_case::GetEffectiveScpsUseCase;
use crate::features::get_effective_scps::mocks::{MockScpRepositoryPort, MockAccountRepositoryPort, MockOuRepositoryPort};
use crate::shared::domain::{ServiceControlPolicy, Account, OrganizationalUnit};
use policies::shared::domain::hrn::Hrn;

#[tokio::test]
async fn test_get_effective_scps_for_account() {
    // Arrange
    let scp_repository = MockScpRepositoryPort::new();
    let account_repository = MockAccountRepositoryPort::new();
    let ou_repository = MockOuRepositoryPort::new();
    
    // Create test entities
    let account_hrn = Hrn::new("account", "test-account");
    let parent_ou_hrn = Hrn::new("ou", "parent-ou");
    let scp_hrn = Hrn::new("scp", "test-scp");
    
    let account = Account::new(
        account_hrn.clone(),
        "TestAccount".to_string(),
        parent_ou_hrn.clone(),
    ).with_attached_scp(scp_hrn.clone());
    
    // Populate mocks
    account_repository.with_account(account);
    
    // Create use case
    let use_case = GetEffectiveScpsUseCase::new(scp_repository, account_repository, ou_repository);
    
    // Create command
    let command = GetEffectiveScpsCommand {
        target_hrn: account_hrn.to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let effective_scps_view = result.unwrap();
    assert_eq!(effective_scps_view.target_hrn, account_hrn.to_string());
    assert_eq!(effective_scps_view.effective_scps, vec![scp_hrn.to_string()]);
}

#[tokio::test]
async fn test_get_effective_scps_for_ou() {
    // Arrange
    let scp_repository = MockScpRepositoryPort::new();
    let account_repository = MockAccountRepositoryPort::new();
    let ou_repository = MockOuRepositoryPort::new();
    
    // Create test entities
    let ou_hrn = Hrn::new("ou", "test-ou");
    let parent_ou_hrn = Hrn::new("ou", "parent-ou");
    let scp_hrn = Hrn::new("scp", "test-scp");
    
    let ou = OrganizationalUnit::new(
        ou_hrn.clone(),
        "TestOU".to_string(),
        parent_ou_hrn.clone(),
    ).with_attached_scp(scp_hrn.clone());
    
    // Populate mocks
    ou_repository.with_ou(ou);
    
    // Create use case
    let use_case = GetEffectiveScpsUseCase::new(scp_repository, account_repository, ou_repository);
    
    // Create command
    let command = GetEffectiveScpsCommand {
        target_hrn: ou_hrn.to_string(),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let effective_scps_view = result.unwrap();
    assert_eq!(effective_scps_view.target_hrn, ou_hrn.to_string());
    assert_eq!(effective_scps_view.effective_scps, vec![scp_hrn.to_string()]);
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/adapter.rs">
use async_trait::async_trait;
use crate::features::move_account::ports::MoveAccountUnitOfWork;
use crate::features::move_account::error::MoveAccountError;
use shared::application::ports::unit_of_work::UnitOfWork;

pub struct MoveAccountSurrealUnitOfWorkAdapter {
    inner_uow: crate::shared::infrastructure::surreal::SurrealUnitOfWork,
}

impl MoveAccountSurrealUnitOfWorkAdapter {
    pub fn new(uow: crate::shared::infrastructure::surreal::SurrealUnitOfWork) -> Self {
        Self {
            inner_uow: uow,
        }
    }
}

#[async_trait]
impl MoveAccountUnitOfWork for MoveAccountSurrealUnitOfWorkAdapter {
    async fn begin(&mut self) -> Result<(), MoveAccountError> {
        self.inner_uow.begin().await
            .map_err(|e| MoveAccountError::OuRepositoryError(crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError(e.to_string())))
    }

    async fn commit(&mut self) -> Result<(), MoveAccountError> {
        self.inner_uow.commit().await
            .map_err(|e| MoveAccountError::OuRepositoryError(crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError(e.to_string())))
    }

    async fn rollback(&mut self) -> Result<(), MoveAccountError> {
        self.inner_uow.rollback().await
            .map_err(|e| MoveAccountError::OuRepositoryError(crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError(e.to_string())))
    }

    fn accounts(&self) -> std::sync::Arc<dyn crate::shared::application::ports::account_repository::AccountRepository> {
        // Note: This is a simplified implementation that would need proper adaptation
        // based on the actual SurrealUnitOfWork implementation
        unimplemented!("Needs proper implementation based on SurrealUnitOfWork")
    }

    fn ous(&self) -> std::sync::Arc<dyn crate::shared::application::ports::ou_repository::OuRepository> {
        // Note: This is a simplified implementation that would need proper adaptation
        // based on the actual SurrealUnitOfWork implementation
        unimplemented!("Needs proper implementation based on SurrealUnitOfWork")
    }
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/di.rs">
use crate::shared::application::ports::{AccountRepository, OuRepository};
use crate::features::move_account::use_case::MoveAccountUseCase;
use crate::features::move_account::adapter::{AccountRepositoryAdapter, OuRepositoryAdapter};

/// Create an instance of the MoveAccountUseCase with the provided repositories
pub fn move_account_use_case<AR: AccountRepository, OR: OuRepository>(
    account_repository: AR,
    ou_repository: OR,
) -> MoveAccountUseCase<AccountRepositoryAdapter<AR>, OuRepositoryAdapter<OR>> {
    let account_adapter = AccountRepositoryAdapter::new(account_repository);
    let ou_adapter = OuRepositoryAdapter::new(ou_repository);
    MoveAccountUseCase::new(account_adapter, ou_adapter)
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/dto.rs">
use serde::{Deserialize, Serialize};
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveAccountCommand {
    pub account_hrn: Hrn,
    pub source_ou_hrn: Hrn,
    pub target_ou_hrn: Hrn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountView {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/error.rs">
use thiserror::Error;
use crate::shared::application::ports::account_repository::AccountRepositoryError;
use crate::shared::application::ports::ou_repository::OuRepositoryError;

#[derive(Debug, Error)]
pub enum MoveAccountError {
    #[error("Account repository error: {0}")]
    AccountRepositoryError(#[from] AccountRepositoryError),
    #[error("OU repository error: {0}")]
    OuRepositoryError(#[from] OuRepositoryError),
    #[error("Account not found")]
    AccountNotFound,
    #[error("Source OU not found")]
    SourceOuNotFound,
    #[error("Target OU not found")]
    TargetOuNotFound,
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/mod.rs">
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;
pub mod adapter;
pub mod mocks;

#[cfg(test)]
pub mod use_case_test;

// Re-export the main types for easier use
pub use use_case::MoveAccountUseCase;
pub use dto::MoveAccountCommand;
pub use error::MoveAccountError;
pub use ports::{MoveAccountUnitOfWorkFactory, MoveAccountUnitOfWork};
pub use adapter::MoveAccountSurrealUnitOfWorkAdapter;
</file>

<file path="crates/hodei-organizations/src/features/move_account/ports.rs">
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;
use crate::features::move_account::error::MoveAccountError;
use policies::domain::Hrn;
use async_trait::async_trait;
use std::sync::Arc;

/// Simplified UnitOfWork trait for MoveAccountUseCase
/// 
/// This trait provides direct access to the generic UnitOfWork interface
/// to avoid complex adapter patterns.
#[async_trait]
pub trait MoveAccountUnitOfWork: Send + Sync {
    /// Begin a new transaction
    async fn begin(&mut self) -> Result<(), MoveAccountError>;
    
    /// Commit the current transaction
    async fn commit(&mut self) -> Result<(), MoveAccountError>;
    
    /// Rollback the current transaction
    async fn rollback(&mut self) -> Result<(), MoveAccountError>;
    
    /// Get account repository for this transaction
    fn accounts(&self) -> Arc<dyn crate::shared::application::ports::account_repository::AccountRepository>;
    
    /// Get OU repository for this transaction
    fn ous(&self) -> Arc<dyn crate::shared::application::ports::ou_repository::OuRepository>;
}

/// Simplified UnitOfWorkFactory trait for MoveAccountUseCase
#[async_trait]
pub trait MoveAccountUnitOfWorkFactory: Send + Sync {
    /// Type of UnitOfWork this factory creates
    type UnitOfWork: MoveAccountUnitOfWork;
    
    /// Create a new UnitOfWork instance
    async fn create(&self) -> Result<Self::UnitOfWork, MoveAccountError>;
}

// Legacy traits for backward compatibility during migration
#[async_trait]
pub trait AccountRepository {
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, MoveAccountError>;
    async fn save(&self, account: Account) -> Result<(), MoveAccountError>;
}

#[async_trait]
pub trait OuRepository {
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, MoveAccountError>;
    async fn save(&self, ou: OrganizationalUnit) -> Result<(), MoveAccountError>;
}
</file>

<file path="crates/hodei-organizations/src/features/move_account/use_case_test.rs">
use std::sync::Arc;
use policies::shared::domain::hrn::Hrn;

use crate::features::move_account::use_case::MoveAccountUseCase;
use crate::features::move_account::dto::MoveAccountCommand;
use crate::features::move_account::mocks::{MockMoveAccountUnitOfWorkFactory};

// Helper function to create test HRNs
fn create_test_hrn(resource_type: &str, resource_id: &str) -> Hrn {
    Hrn::new(
        "aws".to_string(),
        "hodei".to_string(),
        "123456789012".to_string(),
        resource_type.to_string(),
        resource_id.to_string(),
    )
}

#[tokio::test]
async fn test_move_account_successful_transaction() {
    // Arrange
    let mock_factory = Arc::new(MockMoveAccountUnitOfWorkFactory::new());
    let use_case = MoveAccountUseCase::new(mock_factory.clone());
    
    let command = MoveAccountCommand {
        account_hrn: create_test_hrn("account", "test"),
        source_ou_hrn: create_test_hrn("ou", "source"),
        target_ou_hrn: create_test_hrn("ou", "target"),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok(), "Move account should succeed");
}

#[tokio::test]
async fn test_move_account_with_repository_failure_rolls_back() {
    // Arrange
    let mock_factory = Arc::new(MockMoveAccountUnitOfWorkFactory::with_failure(true));
    let use_case = MoveAccountUseCase::new(mock_factory.clone());
    
    let command = MoveAccountCommand {
        account_hrn: create_test_hrn("account", "test"),
        source_ou_hrn: create_test_hrn("ou", "source"),
        target_ou_hrn: create_test_hrn("ou", "target"),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_err(), "Move account should fail when repository fails");
    
    // Verify that the error is propagated correctly
    let err = result.unwrap_err();
    match err {
        crate::features::move_account::error::MoveAccountError::AccountRepositoryError(ref msg) => {
            let msg_str = msg.to_string();
            assert!(msg_str.contains("Mock save failure") || msg_str.contains("Database error"));
        }
        other => panic!("Expected RepositoryError, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_move_account_account_not_found() {
    // Arrange
    let mock_factory = Arc::new(MockMoveAccountUnitOfWorkFactory::new());
    let use_case = MoveAccountUseCase::new(mock_factory.clone());
    
    let command = MoveAccountCommand {
        account_hrn: create_test_hrn("account", "nonexistent"),
        source_ou_hrn: create_test_hrn("ou", "source"),
        target_ou_hrn: create_test_hrn("ou", "target"),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_err(), "Move account should fail when account not found");
    
    let error = result.unwrap_err();
    match error {
        crate::features::move_account::error::MoveAccountError::AccountNotFound => {
            // Expected error
        }
        _ => panic!("Expected AccountNotFound error, got: {:?}", error),
    }
}

#[tokio::test]
async fn test_move_account_source_ou_not_found() {
    // Arrange
    let mock_factory = Arc::new(MockMoveAccountUnitOfWorkFactory::new());
    let use_case = MoveAccountUseCase::new(mock_factory.clone());
    
    let command = MoveAccountCommand {
        account_hrn: create_test_hrn("account", "test"),
        source_ou_hrn: create_test_hrn("ou", "nonexistent"),
        target_ou_hrn: create_test_hrn("ou", "target"),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_err(), "Move account should fail when source OU not found");
    
    let error = result.unwrap_err();
    match error {
        crate::features::move_account::error::MoveAccountError::SourceOuNotFound => {
            // Expected error
        }
        _ => panic!("Expected SourceOuNotFound error, got: {:?}", error),
    }
}

#[tokio::test]
async fn test_move_account_target_ou_not_found() {
    // Arrange
    let mock_factory = Arc::new(MockMoveAccountUnitOfWorkFactory::new());
    let use_case = MoveAccountUseCase::new(mock_factory.clone());
    
    let command = MoveAccountCommand {
        account_hrn: create_test_hrn("account", "test"),
        source_ou_hrn: create_test_hrn("ou", "source"),
        target_ou_hrn: create_test_hrn("ou", "nonexistent"),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_err(), "Move account should fail when target OU not found");
    
    let error = result.unwrap_err();
    match error {
        crate::features::move_account::error::MoveAccountError::TargetOuNotFound => {
            // Expected error
        }
        _ => panic!("Expected TargetOuNotFound error, got: {:?}", error),
    }
}

#[tokio::test]
async fn test_transaction_atomicity_all_operations_succeed() {
    // This test verifies that when all operations succeed, the transaction is committed
    // and all save operations are called
    
    // Arrange
    let mock_factory = Arc::new(MockMoveAccountUnitOfWorkFactory::new());
    let use_case = MoveAccountUseCase::new(mock_factory.clone());
    
    let command = MoveAccountCommand {
        account_hrn: create_test_hrn("account", "test"),
        source_ou_hrn: create_test_hrn("ou", "source"),
        target_ou_hrn: create_test_hrn("ou", "target"),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok(), "Move account should succeed");
    
    // In a real implementation, we would verify that:
    // 1. The transaction was begun
    // 2. All three save operations were called
    // 3. The transaction was committed
    // 4. No rollback occurred
    
    // For now, we just verify the operation succeeds
    // The mock implementation tracks save calls internally
}

#[tokio::test]
async fn test_transaction_atomicity_failure_rolls_back() {
    // This test verifies that when any operation fails, the transaction is rolled back
    // and no partial changes are persisted
    
    // Arrange
    let mock_factory = Arc::new(MockMoveAccountUnitOfWorkFactory::with_failure(true));
    let use_case = MoveAccountUseCase::new(mock_factory.clone());
    
    let command = MoveAccountCommand {
        account_hrn: create_test_hrn("account", "test"),
        source_ou_hrn: create_test_hrn("ou", "source"),
        target_ou_hrn: create_test_hrn("ou", "target"),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_err(), "Move account should fail when repository fails");
    
    // In a real implementation, we would verify that:
    // 1. The transaction was begun
    // 2. Some save operations may have been called before the failure
    // 3. The transaction was rolled back (not committed)
    // 4. No partial changes were persisted
    
    // For now, we just verify the operation fails and error is handled
    let error = result.unwrap_err();
    match error {
        crate::features::move_account::error::MoveAccountError::AccountRepositoryError(_) => {
            // Expected error type
        }
        _ => panic!("Expected RepositoryError, got: {:?}", error),
    }
}
</file>

<file path="crates/hodei-organizations/src/features/mod.rs">
pub mod create_account;
pub mod create_ou;
pub mod move_account;
pub mod create_scp;
pub mod attach_scp;
pub mod get_effective_scps;
</file>

<file path="crates/hodei-organizations/src/shared/application/ports/account_repository.rs">
use crate::shared::domain::account::Account;
use async_trait::async_trait;
use thiserror::Error;
use policies::domain::Hrn;

#[derive(Debug, Error)]
pub enum AccountRepositoryError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Account not found")]
    AccountNotFound,
}

#[async_trait]
pub trait AccountRepository {
    async fn save(&self, account: &Account) -> Result<(), AccountRepositoryError>;
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError>;
}
</file>

<file path="crates/hodei-organizations/src/shared/application/ports/ou_repository.rs">
use crate::shared::domain::ou::OrganizationalUnit;
use async_trait::async_trait;
use thiserror::Error;
use policies::domain::Hrn;

#[derive(Debug, Error)]
pub enum OuRepositoryError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Organizational Unit not found")]
    OuNotFound,
}

#[async_trait]
pub trait OuRepository {
    async fn save(&self, ou: &OrganizationalUnit) -> Result<(), OuRepositoryError>;
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError>;
}
</file>

<file path="crates/hodei-organizations/src/shared/application/ports/scp_repository.rs">
use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;
use crate::shared::domain::scp::ServiceControlPolicy;

/// Error type for SCP repository operations
#[derive(Debug, thiserror::Error)]
pub enum ScpRepositoryError {
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Service Control Policy not found: {0}")]
    NotFound(String),
}

/// Repository trait for ServiceControlPolicy entities
#[async_trait]
pub trait ScpRepository: Send + Sync {
    /// Save an SCP
    async fn save(&self, scp: &ServiceControlPolicy) -> Result<(), ScpRepositoryError>;
    
    /// Find an SCP by HRN
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError>;
}
</file>

<file path="crates/hodei-organizations/src/shared/application/hierarchy_service.rs">
use crate::shared::domain::{Account, OrganizationalUnit};
use crate::shared::application::ports::{AccountRepository, AccountRepositoryError, OuRepository, OuRepositoryError};
use policies::domain::Hrn;
use std::sync::Arc;

/// Servicio para manejar la jerarquía organizacional
pub struct HierarchyService<AR: AccountRepository, OR: OuRepository> {
    account_repo: Arc<AR>,
    ou_repo: Arc<OR>,
}

impl<AR: AccountRepository, OR: OuRepository> HierarchyService<AR, OR> {
    /// Crea una nueva instancia del servicio
    pub fn new(account_repo: Arc<AR>, ou_repo: Arc<OR>) -> Self {
        Self { account_repo, ou_repo }
    }

    /// Obtiene la cadena completa de OUs desde una cuenta hasta la raíz
    pub async fn get_parent_chain(&self, account_hrn: &Hrn) -> Result<Vec<OrganizationalUnit>, HierarchyError> {
        let mut chain = Vec::new();
        let mut current_hrn = account_hrn.clone();
        
        // Comenzar desde la cuenta
        let account = self.account_repo.find_account_by_hrn(&current_hrn).await?
            .ok_or(HierarchyError::AccountNotFound(current_hrn.clone()))?;
        
        // Ascender por la jerarquía
        current_hrn = account.parent_hrn.clone();
        while let Some(ou) = self.ou_repo.find_ou_by_hrn(&current_hrn).await? {
            chain.push(ou.clone());
            current_hrn = ou.parent_hrn.clone();
        }
        
        Ok(chain)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HierarchyError {
    #[error("Account not found: {0}")]
    AccountNotFound(Hrn),
    #[error("OU repository error: {0}")]
    OuRepository(#[from] OuRepositoryError),
    #[error("Account repository error: {0}")]
    AccountRepository(#[from] AccountRepositoryError),
}
</file>

<file path="crates/hodei-organizations/src/shared/application/mod.rs">
pub mod ports;
</file>

<file path="crates/hodei-organizations/src/shared/domain/events.rs">
//! Domain events for the Organizations bounded context
//!
//! These events represent state changes in the Organizations domain that other
//! bounded contexts might be interested in.

use policies::domain::Hrn;
use serde::{Deserialize, Serialize};
use shared::application::ports::event_bus::DomainEvent;

/// Event emitted when a new account is created
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountCreated {
    /// HRN of the created account
    pub account_hrn: Hrn,
    /// Account name
    pub name: String,
    /// HRN of the parent OU (if any)
    pub parent_hrn: Option<Hrn>,
    /// Timestamp when the account was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for AccountCreated {
    fn event_type(&self) -> &'static str {
        "organizations.account.created"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.account_hrn.to_string())
    }
}

/// Event emitted when an account is moved between organizational units
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountMoved {
    /// HRN of the account that was moved
    pub account_hrn: Hrn,
    /// HRN of the source OU (where it was before)
    pub from_ou_hrn: Option<Hrn>,
    /// HRN of the destination OU (where it is now)
    pub to_ou_hrn: Option<Hrn>,
    /// Timestamp when the account was moved
    pub moved_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for AccountMoved {
    fn event_type(&self) -> &'static str {
        "organizations.account.moved"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.account_hrn.to_string())
    }
}

/// Event emitted when an account is deleted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountDeleted {
    /// HRN of the deleted account
    pub account_hrn: Hrn,
    /// Timestamp when the account was deleted
    pub deleted_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for AccountDeleted {
    fn event_type(&self) -> &'static str {
        "organizations.account.deleted"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.account_hrn.to_string())
    }
}

/// Type of target for SCP attachment
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScpTargetType {
    Account,
    OrganizationalUnit,
    Root,
}

impl std::fmt::Display for ScpTargetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScpTargetType::Account => write!(f, "account"),
            ScpTargetType::OrganizationalUnit => write!(f, "organizational_unit"),
            ScpTargetType::Root => write!(f, "root"),
        }
    }
}

/// Event emitted when a Service Control Policy (SCP) is attached to a target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpAttached {
    /// HRN of the SCP that was attached
    pub scp_hrn: Hrn,
    /// HRN of the target (Account, OU, or Root)
    pub target_hrn: Hrn,
    /// Type of the target
    pub target_type: ScpTargetType,
    /// Timestamp when the SCP was attached
    pub attached_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for ScpAttached {
    fn event_type(&self) -> &'static str {
        "organizations.scp.attached"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.target_hrn.to_string())
    }
}

/// Event emitted when a Service Control Policy (SCP) is detached from a target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpDetached {
    /// HRN of the SCP that was detached
    pub scp_hrn: Hrn,
    /// HRN of the target (Account, OU, or Root)
    pub target_hrn: Hrn,
    /// Type of the target
    pub target_type: ScpTargetType,
    /// Timestamp when the SCP was detached
    pub detached_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for ScpDetached {
    fn event_type(&self) -> &'static str {
        "organizations.scp.detached"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.target_hrn.to_string())
    }
}

/// Event emitted when a new organizational unit is created
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationalUnitCreated {
    /// HRN of the created OU
    pub ou_hrn: Hrn,
    /// OU name
    pub name: String,
    /// HRN of the parent OU (if any, None for root-level OUs)
    pub parent_hrn: Option<Hrn>,
    /// Timestamp when the OU was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for OrganizationalUnitCreated {
    fn event_type(&self) -> &'static str {
        "organizations.ou.created"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.ou_hrn.to_string())
    }
}

/// Event emitted when an organizational unit is deleted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationalUnitDeleted {
    /// HRN of the deleted OU
    pub ou_hrn: Hrn,
    /// Timestamp when the OU was deleted
    pub deleted_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for OrganizationalUnitDeleted {
    fn event_type(&self) -> &'static str {
        "organizations.ou.deleted"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.ou_hrn.to_string())
    }
}

/// Event emitted when a Service Control Policy (SCP) is created
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpCreated {
    /// HRN of the created SCP
    pub scp_hrn: Hrn,
    /// SCP name
    pub name: String,
    /// SCP description (optional)
    pub description: Option<String>,
    /// Timestamp when the SCP was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for ScpCreated {
    fn event_type(&self) -> &'static str {
        "organizations.scp.created"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.scp_hrn.to_string())
    }
}

/// Event emitted when a Service Control Policy (SCP) is updated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpUpdated {
    /// HRN of the updated SCP
    pub scp_hrn: Hrn,
    /// SCP name
    pub name: String,
    /// Timestamp when the SCP was updated
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for ScpUpdated {
    fn event_type(&self) -> &'static str {
        "organizations.scp.updated"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.scp_hrn.to_string())
    }
}

/// Event emitted when a Service Control Policy (SCP) is deleted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpDeleted {
    /// HRN of the deleted SCP
    pub scp_hrn: Hrn,
    /// Timestamp when the SCP was deleted
    pub deleted_at: chrono::DateTime<chrono::Utc>,
}

impl DomainEvent for ScpDeleted {
    fn event_type(&self) -> &'static str {
        "organizations.scp.deleted"
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(self.scp_hrn.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_created_event_type() {
        let hrn = Hrn::new(
            "hodei".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "account".to_string(),
            "acc-123".to_string(),
        );

        let event = AccountCreated {
            account_hrn: hrn.clone(),
            name: "Test Account".to_string(),
            parent_hrn: None,
            created_at: chrono::Utc::now(),
        };

        assert_eq!(event.event_type(), "organizations.account.created");
        assert_eq!(event.aggregate_id(), Some(hrn.to_string()));
    }

    #[test]
    fn test_scp_attached_event_type() {
        let scp_hrn = Hrn::new(
            "hodei".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "scp".to_string(),
            "scp-123".to_string(),
        );

        let target_hrn = Hrn::new(
            "hodei".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "account".to_string(),
            "acc-456".to_string(),
        );

        let event = ScpAttached {
            scp_hrn,
            target_hrn: target_hrn.clone(),
            target_type: ScpTargetType::Account,
            attached_at: chrono::Utc::now(),
        };

        assert_eq!(event.event_type(), "organizations.scp.attached");
        assert_eq!(event.aggregate_id(), Some(target_hrn.to_string()));
    }

    #[test]
    fn test_scp_target_type_display() {
        assert_eq!(ScpTargetType::Account.to_string(), "account");
        assert_eq!(
            ScpTargetType::OrganizationalUnit.to_string(),
            "organizational_unit"
        );
        assert_eq!(ScpTargetType::Root.to_string(), "root");
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/domain/ou_test.rs">
use crate::shared::domain::OrganizationalUnit;
use policies::shared::domain::hrn::Hrn;

#[test]
fn test_ou_add_child_account() {
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );
    
    let account_hrn = Hrn::new("account", "test-account");
    ou.add_child_account(account_hrn.clone());
    
    assert!(ou.child_accounts.contains(&account_hrn.to_string()));
}

#[test]
fn test_ou_remove_child_account() {
    let account_hrn = Hrn::new("account", "test-account");
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );
    
    ou.add_child_account(account_hrn.clone());
    assert!(ou.child_accounts.contains(&account_hrn.to_string()));
    
    ou.remove_child_account(account_hrn.clone());
    assert!(!ou.child_accounts.contains(&account_hrn.to_string()));
}

#[test]
fn test_ou_add_child_ou() {
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );
    
    let child_ou_hrn = Hrn::new("ou", "child-ou");
    ou.add_child_ou(child_ou_hrn.clone());
    
    assert!(ou.child_ous.contains(&child_ou_hrn.to_string()));
}

#[test]
fn test_ou_remove_child_ou() {
    let child_ou_hrn = Hrn::new("ou", "child-ou");
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );
    
    ou.add_child_ou(child_ou_hrn.clone());
    assert!(ou.child_ous.contains(&child_ou_hrn.to_string()));
    
    ou.remove_child_ou(child_ou_hrn.clone());
    assert!(!ou.child_ous.contains(&child_ou_hrn.to_string()));
}

#[test]
fn test_ou_attach_scp() {
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );
    
    let scp_hrn = Hrn::new("scp", "test-scp");
    ou.attach_scp(scp_hrn.clone());
    
    assert!(ou.attached_scps.contains(&scp_hrn.to_string()));
}

#[test]
fn test_ou_detach_scp() {
    let scp_hrn = Hrn::new("scp", "test-scp");
    let mut ou = OrganizationalUnit::new(
        Hrn::new("ou", "test-ou"),
        "Test OU".to_string(),
        Hrn::new("ou", "parent-ou"),
    );
    
    ou.attach_scp(scp_hrn.clone());
    assert!(ou.attached_scps.contains(&scp_hrn.to_string()));
    
    ou.detach_scp(scp_hrn.clone());
    assert!(!ou.attached_scps.contains(&scp_hrn.to_string()));
}
</file>

<file path="crates/hodei-organizations/src/shared/domain/ou.rs">
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationalUnit {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
    pub child_ous: HashSet<Hrn>,
    pub child_accounts: HashSet<Hrn>,
    pub attached_scps: HashSet<Hrn>,
}

impl OrganizationalUnit {
    pub fn new(name: String, parent_hrn: Hrn) -> Self {
        let hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            name.clone(),
        );
        Self {
            hrn,
            name,
            parent_hrn,
            child_ous: HashSet::new(),
            child_accounts: HashSet::new(),
            attached_scps: HashSet::new(),
        }
    }
    
    pub fn add_child_ou(&mut self, child_hrn: Hrn) {
        self.child_ous.insert(child_hrn);
    }
    
    pub fn remove_child_ou(&mut self, child_hrn: &Hrn) {
        self.child_ous.remove(child_hrn);
    }
    
    pub fn add_child_account(&mut self, account_hrn: Hrn) {
        self.child_accounts.insert(account_hrn);
    }
    
    pub fn remove_child_account(&mut self, account_hrn: &Hrn) {
        self.child_accounts.remove(account_hrn);
    }
    
    pub fn attach_scp(&mut self, scp_hrn: Hrn) {
        self.attached_scps.insert(scp_hrn);
    }
    
    pub fn detach_scp(&mut self, scp_hrn: &Hrn) {
        self.attached_scps.remove(scp_hrn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_ou_is_valid() {
        let parent_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "parent-1".to_string(),
        );
        let ou = OrganizationalUnit::new("TestOU".to_string(), parent_hrn.clone());
        
        assert_eq!(ou.name, "TestOU");
        assert_eq!(ou.parent_hrn, parent_hrn);
        assert!(ou.child_ous.is_empty());
        assert!(ou.child_accounts.is_empty());
        assert!(ou.attached_scps.is_empty());
        assert!(!ou.hrn.to_string().is_empty());
    }
    
    #[test]
    fn test_add_child_ou() {
        let mut ou = OrganizationalUnit::new(
            "ParentOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let child_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "child-1".to_string(),
        );
        ou.add_child_ou(child_hrn.clone());
        
        assert!(ou.child_ous.contains(&child_hrn));
        assert_eq!(ou.child_ous.len(), 1);
    }
    
    #[test]
    fn test_remove_child_ou() {
        let mut ou = OrganizationalUnit::new(
            "ParentOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let child_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "child-2".to_string(),
        );
        ou.add_child_ou(child_hrn.clone());
        
        assert!(ou.child_ous.contains(&child_hrn));
        
        ou.remove_child_ou(&child_hrn);
        assert!(!ou.child_ous.contains(&child_hrn));
        assert_eq!(ou.child_ous.len(), 0);
    }
    
    #[test]
    fn test_add_child_account() {
        let mut ou = OrganizationalUnit::new(
            "ParentOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let account_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "account".to_string(),
            "acc-1".to_string(),
        );
        ou.add_child_account(account_hrn.clone());
        
        assert!(ou.child_accounts.contains(&account_hrn));
        assert_eq!(ou.child_accounts.len(), 1);
    }
    
    #[test]
    fn test_remove_child_account() {
        let mut ou = OrganizationalUnit::new(
            "ParentOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let account_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "account".to_string(),
            "acc-2".to_string(),
        );
        ou.add_child_account(account_hrn.clone());
        
        assert!(ou.child_accounts.contains(&account_hrn));
        
        ou.remove_child_account(&account_hrn);
        assert!(!ou.child_accounts.contains(&account_hrn));
        assert_eq!(ou.child_accounts.len(), 0);
    }
    
    #[test]
    fn test_attach_scp() {
        let mut ou = OrganizationalUnit::new(
            "TestOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "scp".to_string(),
            "scp-1".to_string(),
        );
        ou.attach_scp(scp_hrn.clone());
        
        assert!(ou.attached_scps.contains(&scp_hrn));
        assert_eq!(ou.attached_scps.len(), 1);
    }
    
    #[test]
    fn test_detach_scp() {
        let mut ou = OrganizationalUnit::new(
            "TestOU".to_string(),
            Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "default".to_string(),
                "root".to_string(),
                "root-1".to_string(),
            ),
        );
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "scp".to_string(),
            "scp-2".to_string(),
        );
        ou.attach_scp(scp_hrn.clone());
        
        assert!(ou.attached_scps.contains(&scp_hrn));
        
        ou.detach_scp(&scp_hrn);
        assert!(!ou.attached_scps.contains(&scp_hrn));
        assert_eq!(ou.attached_scps.len(), 0);
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/domain/scp_test.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use policies::shared::domain::hrn::Hrn;

#[test]
fn test_scp_new() {
    let hrn = Hrn::new("scp", "test-scp");
    let name = "Test SCP".to_string();
    let document = "permit(principal, action, resource);".to_string();
    
    let scp = ServiceControlPolicy::new(hrn.clone(), name.clone(), document.clone());
    
    assert_eq!(scp.hrn, hrn);
    assert_eq!(scp.name, name);
    assert_eq!(scp.document, document);
}

#[test]
fn test_scp_clone() {
    let hrn = Hrn::new("scp", "test-scp");
    let name = "Test SCP".to_string();
    let document = "permit(principal, action, resource);".to_string();
    
    let scp = ServiceControlPolicy::new(hrn.clone(), name.clone(), document.clone());
    let cloned_scp = scp.clone();
    
    assert_eq!(scp.hrn, cloned_scp.hrn);
    assert_eq!(scp.name, cloned_scp.name);
    assert_eq!(scp.document, cloned_scp.document);
}

#[test]
fn test_scp_debug() {
    let hrn = Hrn::new("scp", "test-scp");
    let name = "Test SCP".to_string();
    let document = "permit(principal, action, resource);".to_string();
    
    let scp = ServiceControlPolicy::new(hrn.clone(), name.clone(), document.clone());
    let debug_str = format!("{:?}", scp);
    
    assert!(debug_str.contains("ServiceControlPolicy"));
    assert!(debug_str.contains("test-scp"));
    assert!(debug_str.contains("Test SCP"));
}
</file>

<file path="crates/hodei-organizations/src/shared/domain/scp.rs">
use policies::shared::domain::hrn::Hrn;
use serde::{Deserialize, Serialize};

/// Represents a Service Control Policy in the organization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceControlPolicy {
    /// Unique identifier for the SCP
    pub hrn: Hrn,
    /// Name of the SCP
    pub name: String,
    /// Policy document in Cedar format
    pub document: String,
}

impl ServiceControlPolicy {
    /// Create a new Service Control Policy
    pub fn new(hrn: Hrn, name: String, document: String) -> Self {
        Self {
            hrn,
            name,
            document,
        }
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/infrastructure/surreal/account_repository.rs">
use crate::shared::application::ports::account_repository::{AccountRepository, AccountRepositoryError};
use crate::shared::domain::account::Account;
use policies::domain::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use async_trait::async_trait;

pub struct SurrealAccountRepository {
    db: Surreal<Db>,
}

impl SurrealAccountRepository {
    pub fn new(db: Surreal<Db>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AccountRepository for SurrealAccountRepository {
    async fn save(&self, account: &Account) -> Result<(), AccountRepositoryError> {
        let hrn_str = account.hrn.to_string();
        let _: Option<Account> = self.db.create(("account", &hrn_str)).content(account.clone()).await
            .map_err(|e| AccountRepositoryError::DatabaseError(e.to_string()))?;
        Ok(())
    }
    
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        let hrn_str = hrn.to_string();
        let result: Option<Account> = self.db.select(("account", &hrn_str)).await
            .map_err(|e| AccountRepositoryError::DatabaseError(e.to_string()))?;
        Ok(result)
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/infrastructure/surreal/mod.rs">
/// SurrealDB infrastructure implementations
/// 
/// This module contains all SurrealDB-specific implementations of
/// repositories and transaction management.

pub mod unit_of_work;
pub mod account_repository;
pub mod ou_repository;
pub mod scp_repository;

// Re-export commonly used types
pub use unit_of_work::{
    SurrealUnitOfWork, 
    SurrealUnitOfWorkFactory,
    TransactionalAccountRepository,
    TransactionalOuRepository,
    TransactionalScpRepository,
};
pub use account_repository::SurrealAccountRepository;
pub use ou_repository::SurrealOuRepository;
pub use scp_repository::SurrealScpRepository;
</file>

<file path="crates/hodei-organizations/src/shared/infrastructure/surreal/organization_boundary_provider.rs">
use crate::features::get_effective_scps::use_case::GetEffectiveScpsUseCase;
use crate::features::get_effective_scps::dto::GetEffectiveScpsCommand;
use crate::features::get_effective_scps::di::get_effective_scps_use_case;
use crate::shared::infrastructure::surreal::{SurrealScpRepository, SurrealAccountRepository, SurrealOuRepository};
use hodei_authorizer::features::evaluate_permissions::ports::OrganizationBoundaryProvider;
use hodei_authorizer::features::evaluate_permissions::error::EvaluatePermissionsError;
use policies::shared::domain::hrn::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use async_trait::async_trait;
use cedar_policy::PolicySet;
use std::str::FromStr;

/// SurrealDB implementation of OrganizationBoundaryProvider
pub struct SurrealOrganizationBoundaryProvider {
    db: Surreal<Any>,
}

impl SurrealOrganizationBoundaryProvider {
    /// Create a new SurrealOrganizationBoundaryProvider instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl OrganizationBoundaryProvider for SurrealOrganizationBoundaryProvider {
    /// Get effective SCPs for a resource account
    async fn get_effective_scps_for(&self, resource_hrn: &Hrn) -> Result<PolicySet, EvaluatePermissionsError> {
        // Create repositories
        let scp_repository = SurrealScpRepository::new(self.db.clone());
        let account_repository = SurrealAccountRepository::new(self.db.clone());
        let ou_repository = SurrealOuRepository::new(self.db.clone());
        
        // Create use case
        let use_case = get_effective_scps_use_case(scp_repository, account_repository, ou_repository);
        
        // Create command
        let command = GetEffectiveScpsCommand {
            target_hrn: resource_hrn.to_string(),
        };
        
        // Execute use case
        let result = use_case.execute(command).await
            .map_err(|e| EvaluatePermissionsError::OrganizationBoundaryProvider(e.to_string()))?;
        
        // Create a new PolicySet for Cedar
        let mut policy_set = PolicySet::new();
        
        // Add each SCP policy to the PolicySet
        for scp_hrn_string in result.effective_scps {
            let scp_hrn = Hrn::from_str(&scp_hrn_string)
                .map_err(|e| EvaluatePermissionsError::OrganizationBoundaryProvider(e.to_string()))?;
            
            // Find the actual SCP object
            let scp_repository = SurrealScpRepository::new(self.db.clone());
            let scp = scp_repository.find_by_hrn(&scp_hrn).await
                .map_err(|e| EvaluatePermissionsError::OrganizationBoundaryProvider(e.to_string()))?
                .ok_or_else(|| EvaluatePermissionsError::OrganizationBoundaryProvider(format!("SCP not found: {}", scp_hrn_string)))?;
            
            // Parse the SCP policy text and add to PolicySet
            let policy = cedar_policy::Policy::from_str(&scp.policy_text)
                .map_err(|e| EvaluatePermissionsError::OrganizationBoundaryProvider(format!("Failed to parse SCP policy: {}", e)))?;
            
            policy_set.add_policy(policy);
        }
        
        Ok(policy_set)
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/infrastructure/surreal/ou_repository.rs">
use crate::shared::application::ports::ou_repository::{OuRepository, OuRepositoryError};
use crate::shared::domain::ou::OrganizationalUnit;
use policies::domain::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use async_trait::async_trait;

pub struct SurrealOuRepository {
    db: Surreal<Any>,
}

impl SurrealOuRepository {
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl OuRepository for SurrealOuRepository {
    async fn save(&self, ou: &OrganizationalUnit) -> Result<(), OuRepositoryError> {
        let hrn_str = ou.hrn.to_string();
        let _: Option<OrganizationalUnit> = self.db.create(("ou", &hrn_str)).content(ou.clone()).await
            .map_err(|e| OuRepositoryError::DatabaseError(e.to_string()))?;
        Ok(())
    }
    
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<OrganizationalUnit>, OuRepositoryError> {
        let hrn_str = hrn.to_string();
        let result: Option<OrganizationalUnit> = self.db.select(("ou", &hrn_str)).await
            .map_err(|e| OuRepositoryError::DatabaseError(e.to_string()))?;
        Ok(result)
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/infrastructure/surreal/scp_repository.rs">
use crate::shared::domain::scp::ServiceControlPolicy;
use crate::shared::application::ports::scp_repository::{ScpRepository, ScpRepositoryError};
use policies::shared::domain::hrn::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::RecordId;

/// SurrealDB implementation of ScpRepository
pub struct SurrealScpRepository {
    db: Surreal<Any>,
}

impl SurrealScpRepository {
    /// Create a new SurrealScpRepository instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl ScpRepository for SurrealScpRepository {
    /// Save a service control policy
    async fn save(&self, scp: &ServiceControlPolicy) -> Result<(), ScpRepositoryError> {
        let hrn_string = scp.hrn.to_string();
        let record_id = RecordId::from(("scp", hrn_string.as_str()));
        
        self.db.update::<Option<ServiceControlPolicy>>(record_id)
            .content(scp.clone())
            .await
            .map_err(|e| ScpRepositoryError::Storage(e.to_string()))?;
        
        Ok(())
    }

    /// Find a service control policy by HRN
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let hrn_string = hrn.to_string();
        let record_id = RecordId::from(("scp", hrn_string.as_str()));
        
        let result = self.db.select::<Option<ServiceControlPolicy>>(record_id)
            .await
            .map_err(|e| ScpRepositoryError::Storage(e.to_string()))?;
        
        Ok(result)
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/infrastructure/surreal/unit_of_work.rs">
use std::sync::Arc;
use async_trait::async_trait;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

use shared::application::ports::unit_of_work::{UnitOfWork, UnitOfWorkFactory, UnitOfWorkError};

use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::application::ports::ou_repository::OuRepository;
use crate::shared::application::ports::scp_repository::ScpRepository;

/// Transactional account repository that operates within a UnitOfWork context
pub struct TransactionalAccountRepository {
    db: Arc<Surreal<Any>>,
}

impl TransactionalAccountRepository {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AccountRepository for TransactionalAccountRepository {
    async fn save(&self, account: &crate::shared::domain::account::Account) -> Result<(), crate::shared::application::ports::account_repository::AccountRepositoryError> {
        let hrn_str = account.hrn.to_string();
        self.db.create::<Option<crate::shared::domain::account::Account>>(("account", &hrn_str)).content(account.clone()).await
            .map_err(|e| crate::shared::application::ports::account_repository::AccountRepositoryError::DatabaseError(e.to_string()))?;
        Ok(())
    }
    
    async fn find_by_hrn(&self, hrn: &policies::domain::Hrn) -> Result<Option<crate::shared::domain::account::Account>, crate::shared::application::ports::account_repository::AccountRepositoryError> {
        let hrn_str = hrn.to_string();
        let result: Option<crate::shared::domain::account::Account> = self.db.select(("account", &hrn_str)).await
            .map_err(|e| crate::shared::application::ports::account_repository::AccountRepositoryError::DatabaseError(e.to_string()))?;
        Ok(result)
    }
}

/// Transactional organizational unit repository that operates within a UnitOfWork context
pub struct TransactionalOuRepository {
    db: Arc<Surreal<Any>>,
}

impl TransactionalOuRepository {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl OuRepository for TransactionalOuRepository {
    async fn save(&self, ou: &crate::shared::domain::ou::OrganizationalUnit) -> Result<(), crate::shared::application::ports::ou_repository::OuRepositoryError> {
        let hrn_str = ou.hrn.to_string();
        self.db.create::<Option<crate::shared::domain::ou::OrganizationalUnit>>(("ou", &hrn_str)).content(ou.clone()).await
            .map_err(|e| crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError(e.to_string()))?;
        Ok(())
    }
    
    async fn find_by_hrn(&self, hrn: &policies::domain::Hrn) -> Result<Option<crate::shared::domain::ou::OrganizationalUnit>, crate::shared::application::ports::ou_repository::OuRepositoryError> {
        let hrn_str = hrn.to_string();
        let result: Option<crate::shared::domain::ou::OrganizationalUnit> = self.db.select(("ou", &hrn_str)).await
            .map_err(|e| crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError(e.to_string()))?;
        Ok(result)
    }
}

/// Transactional service control policy repository that operates within a UnitOfWork context
pub struct TransactionalScpRepository {
    db: Arc<Surreal<Any>>,
}

impl TransactionalScpRepository {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ScpRepository for TransactionalScpRepository {
    async fn save(&self, scp: &crate::shared::domain::scp::ServiceControlPolicy) -> Result<(), crate::shared::application::ports::scp_repository::ScpRepositoryError> {
        let hrn_str = scp.hrn.to_string();
        self.db.create::<Option<crate::shared::domain::scp::ServiceControlPolicy>>(("scp", &hrn_str)).content(scp.clone()).await
            .map_err(|e| crate::shared::application::ports::scp_repository::ScpRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }
    
    async fn find_by_hrn(&self, hrn: &policies::domain::Hrn) -> Result<Option<crate::shared::domain::scp::ServiceControlPolicy>, crate::shared::application::ports::scp_repository::ScpRepositoryError> {
        let hrn_str = hrn.to_string();
        let result: Option<crate::shared::domain::scp::ServiceControlPolicy> = self.db.select(("scp", &hrn_str)).await
            .map_err(|e| crate::shared::application::ports::scp_repository::ScpRepositoryError::Storage(e.to_string()))?;
        Ok(result)
    }
}

/// SurrealDB implementation of UnitOfWork
/// 
/// This implementation manages database transactions and provides transactional
/// repository instances that automatically participate in the transaction context.
pub struct SurrealUnitOfWork {
    db: Arc<Surreal<Any>>,
    transaction_started: bool,
}

impl SurrealUnitOfWork {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self {
            db,
            transaction_started: false,
        }
    }
}

#[async_trait]
impl UnitOfWork for SurrealUnitOfWork {
    type AccountRepository = TransactionalAccountRepository;
    type OuRepository = TransactionalOuRepository;
    type ScpRepository = TransactionalScpRepository;

    async fn begin(&mut self) -> Result<(), UnitOfWorkError> {
        if self.transaction_started {
            return Err(UnitOfWorkError::Transaction("Transaction already started".to_string()));
        }

        self.db.query("BEGIN TRANSACTION;").await
            .map_err(|e| UnitOfWorkError::Transaction(e.to_string()))?;
        
        self.transaction_started = true;
        Ok(())
    }
    
    async fn commit(&mut self) -> Result<(), UnitOfWorkError> {
        if !self.transaction_started {
            return Err(UnitOfWorkError::Transaction("No transaction in progress".to_string()));
        }

        self.db.query("COMMIT TRANSACTION;").await
            .map_err(|e| UnitOfWorkError::CommitFailed(e.to_string()))?;
        
        self.transaction_started = false;
        Ok(())
    }
    
    async fn rollback(&mut self) -> Result<(), UnitOfWorkError> {
        if !self.transaction_started {
            return Err(UnitOfWorkError::Transaction("No transaction in progress".to_string()));
        }

        self.db.query("CANCEL TRANSACTION;").await
            .map_err(|e| UnitOfWorkError::RollbackFailed(e.to_string()))?;
        
        self.transaction_started = false;
        Ok(())
    }
    
    fn accounts(&self) -> Arc<Self::AccountRepository> {
        Arc::new(TransactionalAccountRepository::new(self.db.clone()))
    }
    
    fn ous(&self) -> Arc<Self::OuRepository> {
        Arc::new(TransactionalOuRepository::new(self.db.clone()))
    }
    
    fn scps(&self) -> Arc<Self::ScpRepository> {
        Arc::new(TransactionalScpRepository::new(self.db.clone()))
    }
}

impl Drop for SurrealUnitOfWork {
    fn drop(&mut self) {
        if self.transaction_started {
            // Auto-rollback on drop if transaction is still active
            // Note: This is a best-effort cleanup; in async context, we can't
            // guarantee the rollback completes, but we attempt to cancel
            let db = self.db.clone();
            tokio::spawn(async move {
                let _ = db.query("CANCEL TRANSACTION;").await;
            });
        }
    }
}

/// Factory for creating SurrealUnitOfWork instances
pub struct SurrealUnitOfWorkFactory {
    db: Arc<Surreal<Any>>,
}

impl SurrealUnitOfWorkFactory {
    pub fn new(db: Arc<Surreal<Any>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl UnitOfWorkFactory for SurrealUnitOfWorkFactory {
    type UnitOfWork = SurrealUnitOfWork;

    async fn create(&self) -> Result<Self::UnitOfWork, UnitOfWorkError> {
        Ok(SurrealUnitOfWork::new(self.db.clone()))
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/infrastructure/mod.rs">
pub mod surreal;
</file>

<file path="crates/hodei-organizations/src/shared/mod.rs">
pub mod domain;
pub mod application;
pub mod infrastructure;
</file>

<file path="crates/hodei-organizations/README.md">
# hodei-organizations

Este crate implementa la funcionalidad de gestión de organizaciones, cuentas y políticas de control de servicio (SCPs) siguiendo una arquitectura hexagonal y VSA.

## Estructura

```
src/
├── shared/
│   ├── domain/
│   │   ├── account.rs
│   │   ├── ou.rs
│   │   └── scp.rs
│   ├── application/
│   │   └── ports/
│   │       ├── account_repository.rs
│   │       ├── ou_repository.rs
│   │       └── scp_repository.rs
│   └── infrastructure/
│       └── surreal/
│           ├── account_repository.rs
│           ├── ou_repository.rs
│           └── scp_repository.rs
└── features/
    ├── create_account/
    │   ├── mod.rs
    │   ├── use_case.rs
    │   ├── ports.rs
    │   ├── error.rs
    │   ├── dto.rs
    │   ├── adapter.rs
    │   ├── use_case_test.rs
    │   └── mocks.rs
    ├── create_ou/
    ├── move_account/
    ├── create_scp/
    └── attach_scp/
```

## Features Implementadas

### create_account
Permite crear una nueva cuenta en la organización.

## Próximas Features

- create_ou
- move_account
- create_scp
- attach_scp
</file>

<file path="crates/policies/src/features/batch_eval/di.rs">
use anyhow::Result;

use super::use_case::BatchEvalUseCase;

pub async fn make_use_case_mem() -> Result<BatchEvalUseCase> {
    Ok(BatchEvalUseCase::new())
}
</file>

<file path="crates/policies/src/features/batch_eval/dto.rs">
use serde::{Deserialize, Serialize};

use crate::features::policy_playground::dto::{AuthorizationScenario, EntityDefinition, EvaluationStatistics};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BatchPlaygroundRequest {
    pub policies: Vec<String>,
    pub schema: Option<String>,
    #[serde(default)]
    pub entities: Vec<EntityDefinition>,
    pub scenarios: Vec<AuthorizationScenario>,
    #[serde(default)]
    pub limit_scenarios: Option<usize>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchPlaygroundResponse {
    pub results_count: usize,
    pub statistics: EvaluationStatistics,
}
</file>

<file path="crates/policies/src/features/batch_eval/mod.rs">
pub mod dto;
pub mod use_case;
pub mod di;
</file>

<file path="crates/policies/src/features/policy_analysis/di.rs">
use anyhow::Result;

use super::use_case::AnalyzePoliciesUseCase;

pub async fn make_use_case_mem() -> Result<AnalyzePoliciesUseCase> {
    Ok(AnalyzePoliciesUseCase::new())
}
</file>

<file path="crates/policies/src/features/policy_analysis/dto.rs">
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnalyzePoliciesRequest {
    pub policies: Vec<String>,
    pub schema: Option<String>,
    #[serde(default)]
    pub rules: Vec<AnalysisRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnalysisRule {
    pub id: String,
    /// Example: "no_permit_without_mfa"
    pub kind: String,
    /// Optional data for rule (e.g., action name, resource type)
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalyzePoliciesResponse {
    pub passed: bool,
    #[serde(default)]
    pub violations: Vec<RuleViolation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleViolation {
    pub rule_id: String,
    pub message: String,
}
</file>

<file path="crates/policies/src/features/policy_analysis/mod.rs">
pub mod dto;
pub mod use_case;
pub mod di;
</file>

<file path="crates/policies/src/features/policy_playground/dto.rs">
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlaygroundRequest {
    pub policies: Vec<String>,
    pub schema: Option<String>,
    #[serde(default)]
    pub entities: Vec<EntityDefinition>,
    pub authorization_requests: Vec<AuthorizationScenario>,
    pub options: Option<PlaygroundOptions>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EntityDefinition {
    pub uid: String,
    pub attributes: HashMap<String, serde_json::Value>,
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthorizationScenario {
    pub name: String,
    pub principal: String,
    pub action: String,
    pub resource: String,
    pub context: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlaygroundOptions {
    pub include_diagnostics: bool,
}

impl Default for PlaygroundOptions {
    fn default() -> Self {
        Self { include_diagnostics: true }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaygroundResponse {
    pub policy_validation: PolicyValidationResult,
    pub schema_validation: SchemaValidationResult,
    pub authorization_results: Vec<AuthorizationResult>,
    pub statistics: EvaluationStatistics,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
    pub policies_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaValidationResult {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub entity_types_count: usize,
    pub actions_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorizationResult {
    pub scenario_name: String,
    pub decision: Decision,
    pub determining_policies: Vec<String>,
    pub evaluated_policies: Vec<PolicyEvaluation>,
    pub diagnostics: AuthorizationDiagnostics,
    pub evaluation_time_us: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyEvaluation {
    pub policy_id: String,
    pub result: PolicyResult,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub enum PolicyResult {
    Permit,
    Forbid,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorizationDiagnostics {
    pub reasons: Vec<String>,
    pub errors: Vec<String>,
    pub info: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvaluationStatistics {
    pub total_scenarios: usize,
    pub allow_count: usize,
    pub deny_count: usize,
    pub total_evaluation_time_us: u64,
    pub average_evaluation_time_us: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub message: String,
    pub policy_id: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationWarning {
    pub message: String,
    pub severity: WarningSeverity,
}

#[derive(Debug, Clone, Serialize)]
pub enum WarningSeverity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize)]
pub enum Decision { Allow, Deny }
</file>

<file path="crates/policies/src/features/policy_playground/mod.rs">
pub mod dto;
pub mod use_case;
pub mod di;
</file>

<file path="crates/policies/src/features/policy_playground_traces/di.rs">
use anyhow::Result;

use super::use_case::TracedPlaygroundUseCase;

pub async fn make_use_case_mem() -> Result<TracedPlaygroundUseCase> {
    Ok(TracedPlaygroundUseCase::new())
}
</file>

<file path="crates/policies/src/features/policy_playground_traces/dto.rs">
use serde::{Deserialize, Serialize};

use crate::features::policy_playground::dto as base;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TracedPlaygroundOptions {
    #[serde(default)]
    pub include_policy_traces: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TracedAuthorizationResult {
    pub base: base::AuthorizationResult,
    pub determining_policies: Option<Vec<String>>, // None when not requested or unavailable
    pub evaluated_policies: Option<Vec<base::PolicyEvaluation>>, // idem
}

#[derive(Debug, Clone, Serialize)]
pub struct TracedPlaygroundResponse {
    pub policy_validation: base::PolicyValidationResult,
    pub schema_validation: base::SchemaValidationResult,
    pub authorization_results: Vec<TracedAuthorizationResult>,
    pub statistics: base::EvaluationStatistics,
}
</file>

<file path="crates/policies/src/features/policy_playground_traces/mod.rs">
pub mod dto;
pub mod use_case;
pub mod di;
</file>

<file path="crates/policies/src/features/validate_policy/dto.rs">
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct ValidatePolicyQuery {
    pub policy_content: String,
}

impl ValidatePolicyQuery {
    pub fn new(policy_content: String) -> Self {
        Self { policy_content }
    }

    pub fn validate(&self) -> Result<(), ValidatePolicyValidationError> {
        if self.policy_content.trim().is_empty() {
            return Err(ValidatePolicyValidationError::EmptyContent);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationWarning {
    pub message: String,
    pub severity: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ValidatePolicyValidationError {
    #[error("policy content cannot be empty")]
    EmptyContent,
}
</file>

<file path="crates/policies/src/features/validate_policy/mod.rs">
pub mod dto;
pub mod use_case;
pub mod di;
</file>

<file path="crates/policies/src/shared/domain/entity_utils.rs">
//! Utility functions and helpers for working with HodeiEntity implementations.
//!
//! This module provides helper functions to make it easier to implement the HodeiEntity
//! trait, particularly for converting common Rust types to RestrictedExpression values
//! that can be used as entity attributes in Cedar policies.

use cedar_policy::RestrictedExpression;
use std::collections::{BTreeMap, HashMap};

/// Helper trait for converting common Rust types to RestrictedExpression
///
/// This trait provides convenient methods for converting standard Rust types
/// to RestrictedExpression values that can be used as entity attributes.
pub trait ToRestrictedExpression {
    /// Convert the value to a RestrictedExpression
    fn to_restricted_expr(&self) -> RestrictedExpression;
}

impl ToRestrictedExpression for String {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_string(self.clone())
    }
}

impl ToRestrictedExpression for &str {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_string(self.to_string())
    }
}

impl ToRestrictedExpression for bool {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_bool(*self)
    }
}

impl ToRestrictedExpression for i64 {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_long(*self)
    }
}

impl ToRestrictedExpression for i32 {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_long(*self as i64)
    }
}

impl<T: ToRestrictedExpression> ToRestrictedExpression for Vec<T> {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        let expressions: Vec<RestrictedExpression> =
            self.iter().map(|item| item.to_restricted_expr()).collect();
        RestrictedExpression::new_set(expressions)
    }
}

impl<K, V> ToRestrictedExpression for HashMap<K, V>
where
    K: AsRef<str>,
    V: ToRestrictedExpression,
{
    fn to_restricted_expr(&self) -> RestrictedExpression {
        let map: BTreeMap<String, RestrictedExpression> = self
            .iter()
            .map(|(k, v)| (k.as_ref().to_string(), v.to_restricted_expr()))
            .collect();
        RestrictedExpression::new_record(map).unwrap_or_else(|_| {
            RestrictedExpression::new_string("error_creating_record".to_string())
        })
    }
}

impl<K, V> ToRestrictedExpression for BTreeMap<K, V>
where
    K: AsRef<str>,
    V: ToRestrictedExpression,
{
    fn to_restricted_expr(&self) -> RestrictedExpression {
        let map: BTreeMap<String, RestrictedExpression> = self
            .iter()
            .map(|(k, v)| (k.as_ref().to_string(), v.to_restricted_expr()))
            .collect();
        RestrictedExpression::new_record(map).unwrap_or_else(|_| {
            RestrictedExpression::new_string("error_creating_record".to_string())
        })
    }
}

/// Builder for creating entity attributes map
///
/// This provides a fluent API for building the attributes map required by HodeiEntity.
///
/// # Example
///
/// ```
/// use policies::domain::entity_utils::AttributesBuilder;
///
/// let attributes = AttributesBuilder::new()
///     .attr("name", "Alice")
///     .attr("age", 30i64)
///     .attr("active", true)
///     .attr("tags", vec!["employee", "fulltime"])
///     .build();
/// ```
pub struct AttributesBuilder {
    attributes: HashMap<String, RestrictedExpression>,
}

impl AttributesBuilder {
    /// Create a new AttributesBuilder
    pub fn new() -> Self {
        Self {
            attributes: HashMap::new(),
        }
    }

    /// Add an attribute to the builder
    ///
    /// # Example
    ///
    /// ```
    /// use policies::domain::entity_utils::AttributesBuilder;
    ///
    /// let attributes = AttributesBuilder::new()
    ///     .attr("name", "Alice")
    ///     .attr("age", 30i64)
    ///     .build();
    /// ```
    pub fn attr<T: ToRestrictedExpression>(mut self, name: &str, value: T) -> Self {
        self.attributes
            .insert(name.to_string(), value.to_restricted_expr());
        self
    }

    /// Build the attributes map
    pub fn build(self) -> HashMap<String, RestrictedExpression> {
        self.attributes
    }
}

impl Default for AttributesBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_conversions() {
        let string_expr = "test".to_restricted_expr();
        // RestrictedExpression implements Debug, not Display
        assert!(format!("{:?}", string_expr).contains("test"));

        let bool_expr = true.to_restricted_expr();
        assert!(format!("{:?}", bool_expr).to_lowercase().contains("true"));

        let int_expr = 42i64.to_restricted_expr();
        assert!(format!("{:?}", int_expr).contains("42"));
    }

    #[test]
    fn test_collection_conversions() {
        let vec_expr = vec!["a", "b", "c"].to_restricted_expr();
        let vec_str = format!("{:?}", vec_expr);
        assert!(!vec_str.is_empty());

        let mut map = HashMap::new();
        map.insert("key1", "value1");
        map.insert("key2", "value2");
        let map_expr = map.to_restricted_expr();
        let map_str = format!("{:?}", map_expr);
        assert!(!map_str.is_empty());
    }

    #[test]
    fn test_attributes_builder() {
        let attributes = AttributesBuilder::new()
            .attr("name", "Alice")
            .attr("age", 30i64)
            .attr("active", true)
            .attr("tags", vec!["employee", "fulltime"])
            .build();

        assert_eq!(attributes.len(), 4);
        assert!(attributes.contains_key("name"));
        assert!(attributes.contains_key("age"));
        assert!(attributes.contains_key("active"));
        assert!(attributes.contains_key("tags"));
    }
}
</file>

<file path="crates/policies/src/shared/domain/error.rs">
use crate::shared::domain::ports::StorageError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HodeiPoliciesError {
    #[error("Invalid command: {0}")]
    InvalidCommand(String),

    #[error("Policy with ID '{0}' was not found")]
    NotFound(String),

    #[error("Storage error")]
    Storage(#[from] StorageError), // Automatic conversion from StorageError

    #[error("Error parsing policy: {0}")]
    PolicyParse(String),

    #[error("Policy is invalid according to schema: {0}")]
    PolicyValidation(String),

    #[error("Internal engine error: {0}")]
    Engine(String),
}
</file>

<file path="crates/policies/src/shared/infrastructure/surreal/mod.rs">
pub mod mem_storage;

pub use mem_storage::SurrealMemStorage;

#[cfg(feature = "embedded")]
pub mod embedded_storage;

#[cfg(feature = "embedded")]
pub use embedded_storage::SurrealEmbeddedStorage;
</file>

<file path="crates/shared/src/application/ports/event_bus.rs">
//! Event Bus abstraction for domain-driven event communication
//!
//! This module provides the core traits and types for implementing an event-driven
//! architecture that enables loosely-coupled communication between bounded contexts.

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Debug;
use std::sync::Arc;

/// Marker trait for domain events that can be published through the event bus.
///
/// All domain events must be:
/// - Serializable for transport
/// - Deserializable for consumption
/// - Thread-safe (Send + Sync)
/// - Debuggable for tracing/logging
/// - Static lifetime for storage in collections
pub trait DomainEvent:
    Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static
{
    /// Returns the event type identifier for routing and filtering
    fn event_type(&self) -> &'static str;

    /// Returns the aggregate ID that this event relates to (optional)
    fn aggregate_id(&self) -> Option<String> {
        None
    }
}

/// Envelope wrapper for domain events with metadata.
///
/// Provides context about when and why an event occurred, enabling
/// event sourcing, correlation, and causation tracking.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(bound = "T: DomainEvent")]
pub struct EventEnvelope<T: DomainEvent> {
    /// The actual domain event
    pub event: T,

    /// Unique identifier for this event instance
    pub event_id: uuid::Uuid,

    /// Timestamp when the event occurred
    pub occurred_at: chrono::DateTime<chrono::Utc>,

    /// Correlation ID for tracing related events across services
    pub correlation_id: Option<String>,

    /// Causation ID - the ID of the command/event that caused this event
    pub causation_id: Option<String>,

    /// Optional metadata (e.g., user context, tenant ID, etc.)
    pub metadata: std::collections::HashMap<String, String>,
}

impl<T: DomainEvent> EventEnvelope<T> {
    /// Create a new event envelope with default metadata
    pub fn new(event: T) -> Self {
        Self {
            event,
            event_id: uuid::Uuid::new_v4(),
            occurred_at: chrono::Utc::now(),
            correlation_id: None,
            causation_id: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Create an event envelope with correlation tracking
    pub fn with_correlation(event: T, correlation_id: String) -> Self {
        Self {
            event,
            event_id: uuid::Uuid::new_v4(),
            occurred_at: chrono::Utc::now(),
            correlation_id: Some(correlation_id),
            causation_id: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Add metadata to the envelope
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Trait for publishing domain events to the event bus.
///
/// Implementations should be non-blocking and handle failures gracefully.
/// Publishing is fire-and-forget; subscribers process events asynchronously.
#[async_trait]
pub trait EventPublisher: Send + Sync {
    /// Publish a domain event to all interested subscribers
    ///
    /// # Errors
    ///
    /// Returns an error if the event cannot be serialized or if the
    /// underlying transport fails critically. Transient failures should
    /// be handled by the implementation (retries, dead letter queues, etc.)
    async fn publish<E: DomainEvent>(&self, event: E) -> anyhow::Result<()>;

    /// Publish an event with explicit envelope metadata
    async fn publish_with_envelope<E: DomainEvent>(
        &self,
        envelope: EventEnvelope<E>,
    ) -> anyhow::Result<()>;
}

/// Handler for processing domain events of a specific type.
///
/// Each handler should be focused on a single responsibility (SRP).
/// Handlers are invoked asynchronously and should be idempotent.
#[async_trait]
pub trait EventHandler<E: DomainEvent>: Send + Sync {
    /// Logical name for this handler (used for tracing and metrics)
    fn name(&self) -> &'static str;

    /// Handle a domain event
    ///
    /// # Errors
    ///
    /// Should return an error if the event cannot be processed.
    /// The bus implementation decides whether to retry, log, or
    /// move to a dead letter queue.
    async fn handle(&self, envelope: EventEnvelope<E>) -> anyhow::Result<()>;

    /// Optional: filter to determine if this handler should process the event
    ///
    /// Default implementation returns true (process all events of this type).
    /// Override to implement more granular filtering.
    fn should_handle(&self, _envelope: &EventEnvelope<E>) -> bool {
        true
    }
}

/// Represents an active subscription to events.
///
/// Subscriptions can be cancelled and provide observability.
pub trait Subscription: Send + Sync {
    /// Unique identifier for this subscription
    fn id(&self) -> &str;

    /// Event type that this subscription listens to
    fn event_type(&self) -> &'static str;

    /// Handler name
    fn handler_name(&self) -> &'static str;

    /// Cancel the subscription (stop receiving events)
    fn cancel(&self);

    /// Check if the subscription is still active
    fn is_active(&self) -> bool;
}

/// Main event bus abstraction.
///
/// Combines publishing and subscription capabilities. Implementations
/// can be in-memory (for monoliths/testing) or distributed (NATS, Kafka, etc.)
#[async_trait]
pub trait EventBus: EventPublisher {
    /// Subscribe a handler to events of a specific type
    ///
    /// The handler will be invoked asynchronously for each event.
    /// Returns a subscription handle that can be used to cancel.
    ///
    /// # Type Parameters
    ///
    /// - `E`: The event type to subscribe to
    /// - `H`: The handler implementation
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription cannot be established.
    async fn subscribe<E, H>(&self, handler: Arc<H>) -> anyhow::Result<Arc<dyn Subscription>>
    where
        E: DomainEvent,
        H: EventHandler<E> + 'static;

    /// Get count of active subscriptions (for monitoring)
    fn subscription_count(&self) -> usize;

    /// Get count of active handlers across all event types
    fn handler_count(&self) -> usize;
}

/// Blanket implementation for Arc-wrapped EventPublisher
#[async_trait]
impl<T: EventPublisher> EventPublisher for Arc<T> {
    async fn publish<E: DomainEvent>(&self, event: E) -> anyhow::Result<()> {
        (**self).publish(event).await
    }

    async fn publish_with_envelope<E: DomainEvent>(
        &self,
        envelope: EventEnvelope<E>,
    ) -> anyhow::Result<()> {
        (**self).publish_with_envelope(envelope).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Serialize, serde::Deserialize)]
    struct TestEvent {
        message: String,
    }

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> &'static str {
            "test.event"
        }
    }

    #[test]
    fn test_event_envelope_creation() {
        let event = TestEvent {
            message: "test".to_string(),
        };
        let envelope = EventEnvelope::new(event.clone());

        assert_eq!(envelope.event.message, "test");
        assert!(envelope.correlation_id.is_none());
        assert!(envelope.metadata.is_empty());
    }

    #[test]
    fn test_event_envelope_with_correlation() {
        let event = TestEvent {
            message: "test".to_string(),
        };
        let envelope = EventEnvelope::with_correlation(event, "corr-123".to_string());

        assert_eq!(envelope.correlation_id, Some("corr-123".to_string()));
    }

    #[test]
    fn test_event_envelope_with_metadata() {
        let event = TestEvent {
            message: "test".to_string(),
        };
        let envelope = EventEnvelope::new(event)
            .with_metadata("user_id".to_string(), "user-123".to_string())
            .with_metadata("tenant_id".to_string(), "tenant-456".to_string());

        assert_eq!(envelope.metadata.get("user_id").unwrap(), "user-123");
        assert_eq!(envelope.metadata.get("tenant_id").unwrap(), "tenant-456");
    }
}
</file>

<file path="crates/shared/src/application/ports/unit_of_work.rs">
use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;

/// Error types for UnitOfWork operations
#[derive(Debug, Error)]
pub enum UnitOfWorkError {
    #[error("Transaction error: {0}")]
    Transaction(String),
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Commit failed: {0}")]
    CommitFailed(String),
    #[error("Rollback failed: {0}")]
    RollbackFailed(String),
}



/// Unit of Work trait for managing transactions
/// 
/// This trait establishes a contract for transactional operations across different
/// persistence providers. It follows the Unit of Work pattern to ensure that
/// multiple operations are executed atomically.
/// 
/// ## Design Decision
/// Repositories are obtained from the UnitOfWork itself rather than being passed
/// a transaction handle. This ensures that all repository operations are automatically
/// bound to the transaction context without requiring explicit transaction management
/// in the business logic.
#[async_trait]
pub trait UnitOfWork: Send + Sync {
    /// Type for account repository bound to this transaction
    type AccountRepository: Send + Sync;
    
    /// Type for organizational unit repository bound to this transaction
    type OuRepository: Send + Sync;
    
    /// Type for service control policy repository bound to this transaction
    type ScpRepository: Send + Sync;

    /// Begin a new transaction
    async fn begin(&mut self) -> Result<(), UnitOfWorkError>;
    
    /// Commit the current transaction
    async fn commit(&mut self) -> Result<(), UnitOfWorkError>;
    
    /// Rollback the current transaction
    async fn rollback(&mut self) -> Result<(), UnitOfWorkError>;
    
    /// Get a repository for account operations bound to this transaction
    fn accounts(&self) -> Arc<Self::AccountRepository>;
    
    /// Get a repository for organizational unit operations bound to this transaction
    fn ous(&self) -> Arc<Self::OuRepository>;
    
    /// Get a repository for service control policy operations bound to this transaction
    fn scps(&self) -> Arc<Self::ScpRepository>;
}

/// Factory for creating UnitOfWork instances
/// 
/// This allows dependency injection of UnitOfWork creation while keeping the
/// business logic decoupled from the specific implementation.
#[async_trait]
pub trait UnitOfWorkFactory: Send + Sync {
    /// Type of UnitOfWork this factory creates
    type UnitOfWork: UnitOfWork;
    
    /// Create a new UnitOfWork instance
    async fn create(&self) -> Result<Self::UnitOfWork, UnitOfWorkError>;
}
</file>

<file path="crates/shared/src/application/mod.rs">
//! Application layer for the shared kernel
//!
//! This module contains application-level abstractions and contracts
//! that are shared across different bounded contexts.
pub mod ports;

// Re-export commonly used types
pub use ports::{UnitOfWork, UnitOfWorkError, UnitOfWorkFactory};
</file>

<file path="crates/shared/src/infrastructure/audit/handler_test.rs">
// Tests for AuditEventHandler are in handler.rs using #[cfg(test)]
// This file is a placeholder for future integration tests
</file>

<file path="crates/shared/src/infrastructure/audit/handler.rs">
//! Generic audit event handler that captures all domain events
//!
//! This handler implements a universal EventHandler that can capture
//! any domain event and store it in the audit log for compliance and debugging.

use super::{AuditLog, AuditLogStore};
use crate::application::ports::event_bus::{DomainEvent, EventEnvelope, EventHandler};
use async_trait::async_trait;
use std::sync::Arc;

/// Universal audit event handler that captures all domain events
///
/// This handler is generic over any DomainEvent type and stores
/// the event data as JSON in the audit log.
pub struct AuditEventHandler {
    store: Arc<AuditLogStore>,
}

impl AuditEventHandler {
    /// Create a new audit event handler with the given store
    pub fn new(store: Arc<AuditLogStore>) -> Self {
        Self { store }
    }

    /// Get the underlying store (useful for testing)
    #[cfg(test)]
    pub fn store(&self) -> Arc<AuditLogStore> {
        self.store.clone()
    }
}

/// Implement EventHandler for any DomainEvent type
///
/// This allows a single AuditEventHandler instance to handle
/// events of different types through dynamic dispatch.
#[async_trait]
impl<E: DomainEvent> EventHandler<E> for AuditEventHandler {
    fn name(&self) -> &'static str {
        "AuditEventHandler"
    }

    async fn handle(&self, envelope: EventEnvelope<E>) -> anyhow::Result<()> {
        // Serialize the event to JSON
        let event_data = serde_json::to_value(&envelope.event)?;

        // Extract aggregate type from metadata
        let aggregate_type = envelope.metadata.get("aggregate_type").cloned();

        // Create audit log entry
        let audit_log = AuditLog {
            id: envelope.event_id,
            event_type: envelope.event.event_type().to_string(),
            aggregate_id: envelope.event.aggregate_id(),
            aggregate_type,
            event_data,
            occurred_at: envelope.occurred_at,
            correlation_id: envelope.correlation_id.clone(),
            causation_id: envelope.causation_id.clone(),
            metadata: envelope.metadata.clone(),
        };

        // Store the audit log
        self.store.add(audit_log.clone()).await;

        // Log to tracing for operational visibility
        tracing::info!(
            event_type = %audit_log.event_type,
            event_id = %audit_log.id,
            aggregate_id = ?audit_log.aggregate_id,
            aggregate_type = ?audit_log.aggregate_type,
            "Domain event captured in audit log"
        );

        Ok(())
    }

    fn should_handle(&self, _envelope: &EventEnvelope<E>) -> bool {
        // Capture ALL events - no filtering
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::event_bus::EventEnvelope;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestEvent {
        message: String,
    }

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> &'static str {
            "test.event"
        }

        fn aggregate_id(&self) -> Option<String> {
            Some("test-123".to_string())
        }
    }

    #[tokio::test]
    async fn test_audit_handler_captures_event() {
        let store = Arc::new(AuditLogStore::new());
        let handler = AuditEventHandler::new(store.clone());

        let event = TestEvent {
            message: "Test message".to_string(),
        };

        let envelope = EventEnvelope::new(event)
            .with_metadata("aggregate_type".to_string(), "TestAggregate".to_string());

        let result = handler.handle(envelope).await;
        assert!(result.is_ok());

        let logs = store.all().await;
        assert_eq!(logs.len(), 1);

        let log = &logs[0];
        assert_eq!(log.event_type, "test.event");
        assert_eq!(log.aggregate_id, Some("test-123".to_string()));
        assert_eq!(log.aggregate_type, Some("TestAggregate".to_string()));
    }

    #[tokio::test]
    async fn test_audit_handler_multiple_events() {
        let store = Arc::new(AuditLogStore::new());
        let handler = AuditEventHandler::new(store.clone());

        for i in 0..5 {
            let event = TestEvent {
                message: format!("Message {}", i),
            };
            let envelope = EventEnvelope::new(event);
            handler.handle(envelope).await.unwrap();
        }

        let logs = store.all().await;
        assert_eq!(logs.len(), 5);
    }

    #[tokio::test]
    async fn test_audit_handler_should_handle_all() {
        let store = Arc::new(AuditLogStore::new());
        let handler = AuditEventHandler::new(store);

        let event = TestEvent {
            message: "Test".to_string(),
        };
        let envelope = EventEnvelope::new(event);

        assert!(handler.should_handle(&envelope));
    }
}
</file>

<file path="crates/shared/src/infrastructure/audit/mod.rs">
//! Audit system for capturing and querying domain events
//!
//! This module provides a CloudWatch-like audit logging system that captures
//! all domain events for compliance, debugging, and operational insights.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub mod handler;
pub mod query;

#[cfg(test)]
mod handler_test;
#[cfg(test)]
mod query_test;

// Re-export key types for convenience
pub use handler::AuditEventHandler;
pub use query::AuditQuery;

/// An audit log entry representing a captured domain event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    /// Unique identifier for this audit log entry
    pub id: Uuid,

    /// Type of the event (e.g., "iam.user.created")
    pub event_type: String,

    /// Aggregate ID that this event relates to
    pub aggregate_id: Option<String>,

    /// Type of the aggregate (e.g., "User", "Group", "Account")
    pub aggregate_type: Option<String>,

    /// The event data as JSON
    pub event_data: serde_json::Value,

    /// When the event occurred
    pub occurred_at: DateTime<Utc>,

    /// Correlation ID for tracing related events
    pub correlation_id: Option<String>,

    /// Causation ID - the ID of the command/event that caused this event
    pub causation_id: Option<String>,

    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// In-memory store for audit logs (production would use a database)
#[derive(Clone)]
pub struct AuditLogStore {
    logs: Arc<RwLock<Vec<AuditLog>>>,
}

impl AuditLogStore {
    /// Create a new empty audit log store
    pub fn new() -> Self {
        Self {
            logs: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a new audit log entry
    pub async fn add(&self, log: AuditLog) {
        let mut logs = self.logs.write().await;
        logs.push(log);
    }

    /// Get all audit logs (use query() for filtering)
    pub async fn all(&self) -> Vec<AuditLog> {
        let logs = self.logs.read().await;
        logs.clone()
    }

    /// Get a specific audit log by ID
    pub async fn get_by_id(&self, id: Uuid) -> Option<AuditLog> {
        let logs = self.logs.read().await;
        logs.iter().find(|log| log.id == id).cloned()
    }

    /// Count total audit logs
    pub async fn count_all(&self) -> usize {
        let logs = self.logs.read().await;
        logs.len()
    }

    /// Clear all logs (useful for testing)
    #[cfg(test)]
    pub async fn clear(&self) {
        let mut logs = self.logs.write().await;
        logs.clear();
    }
}

impl Default for AuditLogStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about audit logs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditStats {
    pub total_events: usize,
    pub events_by_type: HashMap<String, usize>,
    pub events_by_aggregate_type: HashMap<String, usize>,
    pub oldest_event: Option<DateTime<Utc>>,
    pub newest_event: Option<DateTime<Utc>>,
}

impl AuditLogStore {
    /// Get statistics about the audit logs
    pub async fn stats(&self) -> AuditStats {
        let logs = self.logs.read().await;

        let mut events_by_type: HashMap<String, usize> = HashMap::new();
        let mut events_by_aggregate_type: HashMap<String, usize> = HashMap::new();
        let mut oldest: Option<DateTime<Utc>> = None;
        let mut newest: Option<DateTime<Utc>> = None;

        for log in logs.iter() {
            // Count by event type
            *events_by_type.entry(log.event_type.clone()).or_insert(0) += 1;

            // Count by aggregate type
            if let Some(ref agg_type) = log.aggregate_type {
                *events_by_aggregate_type
                    .entry(agg_type.clone())
                    .or_insert(0) += 1;
            }

            // Track oldest and newest
            if oldest.is_none() || log.occurred_at < oldest.unwrap() {
                oldest = Some(log.occurred_at);
            }
            if newest.is_none() || log.occurred_at > newest.unwrap() {
                newest = Some(log.occurred_at);
            }
        }

        AuditStats {
            total_events: logs.len(),
            events_by_type,
            events_by_aggregate_type,
            oldest_event: oldest,
            newest_event: newest,
        }
    }
}
</file>

<file path="crates/shared/src/infrastructure/audit/query_test.rs">
// Tests for query module are in query.rs using #[cfg(test)]
// This file is a placeholder for future integration tests
</file>

<file path="crates/shared/src/infrastructure/audit/query.rs">
//! Query API for filtering and retrieving audit logs
//!
//! This module provides a flexible query interface for searching
//! audit logs, similar to AWS CloudWatch Logs Insights.

use super::{AuditLog, AuditLogStore};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Query parameters for filtering audit logs
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditQuery {
    /// Filter by event type (exact match)
    pub event_type: Option<String>,

    /// Filter by aggregate ID (exact match)
    pub aggregate_id: Option<String>,

    /// Filter by aggregate type (exact match)
    pub aggregate_type: Option<String>,

    /// Filter events that occurred after this time (inclusive)
    pub from_date: Option<DateTime<Utc>>,

    /// Filter events that occurred before this time (inclusive)
    pub to_date: Option<DateTime<Utc>>,

    /// Filter by correlation ID
    pub correlation_id: Option<String>,

    /// Maximum number of results to return
    pub limit: Option<usize>,

    /// Number of results to skip (for pagination)
    pub offset: Option<usize>,
}

impl AuditQuery {
    /// Create a new empty query
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by event type
    pub fn with_event_type(mut self, event_type: impl Into<String>) -> Self {
        self.event_type = Some(event_type.into());
        self
    }

    /// Filter by aggregate ID
    pub fn with_aggregate_id(mut self, aggregate_id: impl Into<String>) -> Self {
        self.aggregate_id = Some(aggregate_id.into());
        self
    }

    /// Filter by aggregate type
    pub fn with_aggregate_type(mut self, aggregate_type: impl Into<String>) -> Self {
        self.aggregate_type = Some(aggregate_type.into());
        self
    }

    /// Filter by date range
    pub fn with_date_range(mut self, from: DateTime<Utc>, to: DateTime<Utc>) -> Self {
        self.from_date = Some(from);
        self.to_date = Some(to);
        self
    }

    /// Filter by correlation ID
    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    /// Limit the number of results
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set pagination offset
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Check if a log matches this query
    fn matches(&self, log: &AuditLog) -> bool {
        // Filter by event type
        if let Some(ref event_type) = self.event_type
            && &log.event_type != event_type
        {
            return false;
        }

        // Filter by aggregate ID
        if let Some(ref aggregate_id) = self.aggregate_id
            && log.aggregate_id.as_ref() != Some(aggregate_id)
        {
            return false;
        }

        // Filter by aggregate type
        if let Some(ref aggregate_type) = self.aggregate_type
            && log.aggregate_type.as_ref() != Some(aggregate_type)
        {
            return false;
        }

        // Filter by date range
        if let Some(from_date) = self.from_date
            && log.occurred_at < from_date
        {
            return false;
        }

        if let Some(to_date) = self.to_date
            && log.occurred_at > to_date
        {
            return false;
        }

        // Filter by correlation ID
        if let Some(ref correlation_id) = self.correlation_id
            && log.correlation_id.as_ref() != Some(correlation_id)
        {
            return false;
        }

        true
    }
}

impl AuditLogStore {
    /// Query audit logs with filters
    pub async fn query(&self, query: AuditQuery) -> Vec<AuditLog> {
        let logs = self.logs.read().await;

        let mut results: Vec<AuditLog> = logs
            .iter()
            .filter(|log| query.matches(log))
            .cloned()
            .collect();

        // Sort by occurred_at descending (newest first)
        results.sort_by(|a, b| b.occurred_at.cmp(&a.occurred_at));

        // Apply pagination
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(usize::MAX);

        results.into_iter().skip(offset).take(limit).collect()
    }

    /// Count audit logs matching the query
    pub async fn count(&self, query: AuditQuery) -> usize {
        let logs = self.logs.read().await;
        logs.iter().filter(|log| query.matches(log)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use uuid::Uuid;

    fn create_test_log(
        event_type: &str,
        aggregate_id: &str,
        aggregate_type: &str,
        occurred_at: DateTime<Utc>,
    ) -> AuditLog {
        AuditLog {
            id: Uuid::new_v4(),
            event_type: event_type.to_string(),
            aggregate_id: Some(aggregate_id.to_string()),
            aggregate_type: Some(aggregate_type.to_string()),
            event_data: serde_json::json!({}),
            occurred_at,
            correlation_id: None,
            causation_id: None,
            metadata: Default::default(),
        }
    }

    #[tokio::test]
    async fn test_query_by_event_type() {
        let store = AuditLogStore::new();
        let now = Utc::now();

        store
            .add(create_test_log("user.created", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("user.updated", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("group.created", "group-1", "Group", now))
            .await;

        let query = AuditQuery::new().with_event_type("user.created");
        let results = store.query(query).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type, "user.created");
    }

    #[tokio::test]
    async fn test_query_by_aggregate_id() {
        let store = AuditLogStore::new();
        let now = Utc::now();

        store
            .add(create_test_log("user.created", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("user.updated", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("user.created", "user-2", "User", now))
            .await;

        let query = AuditQuery::new().with_aggregate_id("user-1");
        let results = store.query(query).await;

        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .all(|r| r.aggregate_id == Some("user-1".to_string()))
        );
    }

    #[tokio::test]
    async fn test_query_by_aggregate_type() {
        let store = AuditLogStore::new();
        let now = Utc::now();

        store
            .add(create_test_log("user.created", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("group.created", "group-1", "Group", now))
            .await;

        let query = AuditQuery::new().with_aggregate_type("User");
        let results = store.query(query).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].aggregate_type, Some("User".to_string()));
    }

    #[tokio::test]
    async fn test_query_by_date_range() {
        let store = AuditLogStore::new();
        let now = Utc::now();
        let one_hour_ago = now - Duration::hours(1);
        let two_hours_ago = now - Duration::hours(2);

        store
            .add(create_test_log("event1", "id-1", "Type1", two_hours_ago))
            .await;
        store
            .add(create_test_log("event2", "id-2", "Type2", one_hour_ago))
            .await;
        store
            .add(create_test_log("event3", "id-3", "Type3", now))
            .await;

        let query = AuditQuery::new().with_date_range(one_hour_ago, now);
        let results = store.query(query).await;

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_query_with_limit() {
        let store = AuditLogStore::new();
        let now = Utc::now();

        for i in 0..10 {
            store
                .add(create_test_log(&format!("event{}", i), "id", "Type", now))
                .await;
        }

        let query = AuditQuery::new().with_limit(5);
        let results = store.query(query).await;

        assert_eq!(results.len(), 5);
    }

    #[tokio::test]
    async fn test_query_with_offset() {
        let store = AuditLogStore::new();
        let now = Utc::now();

        for i in 0..10 {
            store
                .add(create_test_log(&format!("event{}", i), "id", "Type", now))
                .await;
        }

        let query = AuditQuery::new().with_offset(5).with_limit(3);
        let results = store.query(query).await;

        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_query_count() {
        let store = AuditLogStore::new();
        let now = Utc::now();

        store
            .add(create_test_log("user.created", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("user.created", "user-2", "User", now))
            .await;
        store
            .add(create_test_log("group.created", "group-1", "Group", now))
            .await;

        let query = AuditQuery::new().with_event_type("user.created");
        let count = store.count(query).await;

        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_query_combined_filters() {
        let store = AuditLogStore::new();
        let now = Utc::now();
        let one_hour_ago = now - Duration::hours(1);

        store
            .add(create_test_log(
                "user.created",
                "user-1",
                "User",
                one_hour_ago,
            ))
            .await;
        store
            .add(create_test_log("user.updated", "user-1", "User", now))
            .await;
        store
            .add(create_test_log("user.created", "user-2", "User", now))
            .await;

        let query = AuditQuery::new()
            .with_event_type("user.created")
            .with_date_range(one_hour_ago, now)
            .with_limit(10);

        let results = store.query(query).await;

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.event_type == "user.created"));
    }
}
</file>

<file path="crates/shared/src/infrastructure/in_memory_event_bus.rs">
//! In-memory event bus implementation using tokio broadcast channels
//!
//! This implementation is suitable for:
//! - Monolithic deployments
//! - Development and testing
//! - Local event-driven architectures
//!
//! For distributed systems, use a message broker adapter (NATS, Kafka, etc.)

use crate::application::ports::event_bus::{
    DomainEvent, EventBus, EventEnvelope, EventHandler, EventPublisher, Subscription,
};
use async_trait::async_trait;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Internal representation of a channel for a specific event type
struct TypedChannel {
    sender: broadcast::Sender<Vec<u8>>,
}

/// In-memory event bus using tokio broadcast channels
///
/// Each event type gets its own broadcast channel. Handlers subscribe
/// to specific event types and receive events asynchronously via spawned tasks.
///
/// # Performance Characteristics
///
/// - Publishing: O(1) - just sends to broadcast channel
/// - Fan-out: Automatic via broadcast (each subscriber gets a copy)
/// - Lagging: Subscribers that can't keep up will skip events (logged as warning)
/// - Memory: Bounded channel size (default 1024 events per type)
///
/// # Thread Safety
///
/// All operations are thread-safe and can be called from multiple tasks concurrently.
pub struct InMemoryEventBus {
    /// Map of TypeId -> broadcast channel for each event type
    channels: RwLock<HashMap<TypeId, TypedChannel>>,

    /// Active subscriptions count (for monitoring)
    subscription_count: Arc<std::sync::atomic::AtomicUsize>,

    /// Channel capacity per event type
    channel_capacity: usize,
}

impl InMemoryEventBus {
    /// Create a new in-memory event bus with default capacity (1024)
    pub fn new() -> Self {
        Self::with_capacity(1024)
    }

    /// Create a new in-memory event bus with specified channel capacity
    ///
    /// # Arguments
    ///
    /// * `capacity` - Number of events to buffer per event type channel
    ///
    /// # Recommendations
    ///
    /// - For high-throughput: 2048 or higher
    /// - For low-latency: 256 or lower
    /// - For testing: 16 (makes lag scenarios easier to trigger)
    pub fn with_capacity(capacity: usize) -> Self {
        info!("Creating InMemoryEventBus with capacity {}", capacity);
        Self {
            channels: RwLock::new(HashMap::new()),
            subscription_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            channel_capacity: capacity,
        }
    }

    /// Get or create a broadcast channel for a specific event type
    fn get_or_create_channel<E: DomainEvent>(&self) -> broadcast::Sender<Vec<u8>> {
        let type_id = TypeId::of::<E>();

        // Fast path: channel already exists
        {
            let channels = self.channels.read().unwrap();
            if let Some(channel) = channels.get(&type_id) {
                return channel.sender.clone();
            }
        }

        // Slow path: create new channel
        let mut channels = self.channels.write().unwrap();

        // Double-check in case another thread created it
        if let Some(channel) = channels.get(&type_id) {
            return channel.sender.clone();
        }

        let (tx, _rx) = broadcast::channel::<Vec<u8>>(self.channel_capacity);
        let event_type = std::any::type_name::<E>();

        debug!(
            "Created new broadcast channel for event type: {}",
            event_type
        );

        channels.insert(type_id, TypedChannel { sender: tx.clone() });

        tx
    }
}

impl Default for InMemoryEventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventPublisher for InMemoryEventBus {
    async fn publish<E: DomainEvent>(&self, event: E) -> anyhow::Result<()> {
        let envelope = EventEnvelope::new(event);
        self.publish_with_envelope(envelope).await
    }

    async fn publish_with_envelope<E: DomainEvent>(
        &self,
        envelope: EventEnvelope<E>,
    ) -> anyhow::Result<()> {
        let event_type = envelope.event.event_type();

        debug!(
            event_type = event_type,
            event_id = %envelope.event_id,
            "Publishing event"
        );

        // Serialize the envelope
        let bytes = bincode::serialize(&envelope)
            .map_err(|e| anyhow::anyhow!("Failed to serialize event envelope: {}", e))?;

        // Get the channel and send
        let sender = self.get_or_create_channel::<E>();
        let receiver_count = sender.receiver_count();

        if receiver_count == 0 {
            debug!(
                event_type = event_type,
                "No subscribers for event type, event will be dropped"
            );
        }

        // Send returns error only if there are no receivers (which is fine)
        let _ = sender.send(bytes);

        debug!(
            event_type = event_type,
            event_id = %envelope.event_id,
            receivers = receiver_count,
            "Event published"
        );

        Ok(())
    }
}

#[async_trait]
impl EventBus for InMemoryEventBus {
    async fn subscribe<E, H>(&self, handler: Arc<H>) -> anyhow::Result<Arc<dyn Subscription>>
    where
        E: DomainEvent,
        H: EventHandler<E> + 'static,
    {
        let sender = self.get_or_create_channel::<E>();
        let mut receiver = sender.subscribe();
        let handler_name = handler.name();
        let event_type_name = std::any::type_name::<E>();

        info!(
            handler = handler_name,
            event_type = event_type_name,
            "Subscribing handler to event type"
        );

        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();
        let subscription_id = format!("{}-{}", handler_name, uuid::Uuid::new_v4());
        let is_active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let is_active_clone = is_active.clone();

        // Increment subscription count
        self.subscription_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let sub_count_clone = self.subscription_count.clone();

        // Spawn task to handle incoming events
        let task: JoinHandle<()> = tokio::spawn(async move {
            let mut processed_count = 0u64;
            let mut error_count = 0u64;
            let mut lagged_count = 0u64;

            loop {
                tokio::select! {
                    biased;

                    // Check for cancellation first
                    _ = &mut cancel_rx => {
                        info!(
                            handler = handler_name,
                            processed = processed_count,
                            errors = error_count,
                            lagged = lagged_count,
                            "Handler subscription cancelled"
                        );
                        break;
                    }

                    // Receive event
                    msg = receiver.recv() => {
                        match msg {
                            Ok(bytes) => {
                                // Deserialize envelope
                                match bincode::deserialize::<EventEnvelope<E>>(&bytes) {
                                    Ok(envelope) => {
                                        // Check if handler wants to process this event
                                        if !handler.should_handle(&envelope) {
                                            debug!(
                                                handler = handler_name,
                                                event_id = %envelope.event_id,
                                                "Handler filtered out event"
                                            );
                                            continue;
                                        }

                                        // Handle the event
                                        match handler.handle(envelope.clone()).await {
                                            Ok(_) => {
                                                processed_count += 1;
                                                debug!(
                                                    handler = handler_name,
                                                    event_id = %envelope.event_id,
                                                    processed = processed_count,
                                                    "Event handled successfully"
                                                );
                                            }
                                            Err(e) => {
                                                error_count += 1;
                                                error!(
                                                    handler = handler_name,
                                                    event_id = %envelope.event_id,
                                                    error = %e,
                                                    errors = error_count,
                                                    "Handler failed to process event"
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error_count += 1;
                                        error!(
                                            handler = handler_name,
                                            error = %e,
                                            "Failed to deserialize event envelope"
                                        );
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                lagged_count += skipped;
                                warn!(
                                    handler = handler_name,
                                    skipped = skipped,
                                    total_lagged = lagged_count,
                                    "Handler lagged behind, events were skipped"
                                );
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                info!(
                                    handler = handler_name,
                                    "Event channel closed, stopping handler"
                                );
                                break;
                            }
                        }
                    }
                }
            }

            // Mark as inactive when task completes
            is_active_clone.store(false, std::sync::atomic::Ordering::Relaxed);
            sub_count_clone.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        });

        // Create subscription handle
        let subscription = Arc::new(InMemorySubscription {
            id: subscription_id,
            event_type: event_type_name,
            handler_name,
            cancel_tx: tokio::sync::Mutex::new(Some(cancel_tx)),
            is_active,
            _task: task,
        });

        Ok(subscription as Arc<dyn Subscription>)
    }

    fn subscription_count(&self) -> usize {
        self.subscription_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn handler_count(&self) -> usize {
        self.subscription_count()
    }
}

/// Implementation of Subscription for in-memory subscriptions
struct InMemorySubscription {
    id: String,
    event_type: &'static str,
    handler_name: &'static str,
    cancel_tx: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    is_active: Arc<std::sync::atomic::AtomicBool>,
    _task: JoinHandle<()>,
}

impl Subscription for InMemorySubscription {
    fn id(&self) -> &str {
        &self.id
    }

    fn event_type(&self) -> &'static str {
        self.event_type
    }

    fn handler_name(&self) -> &'static str {
        self.handler_name
    }

    fn cancel(&self) {
        info!(
            subscription_id = self.id,
            handler = self.handler_name,
            "Cancelling subscription"
        );

        // Try to acquire lock without blocking
        if let Ok(mut guard) = self.cancel_tx.try_lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(());
                self.is_active
                    .store(false, std::sync::atomic::Ordering::Relaxed);
            }
        } else {
            // If we can't get the lock, mark as inactive anyway
            self.is_active
                .store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn is_active(&self) -> bool {
        self.is_active.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    struct TestEvent {
        message: String,
    }

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> &'static str {
            "test.event"
        }
    }

    struct TestHandler {
        name: &'static str,
        counter: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl EventHandler<TestEvent> for TestHandler {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn handle(&self, envelope: EventEnvelope<TestEvent>) -> anyhow::Result<()> {
            self.counter.fetch_add(1, Ordering::SeqCst);
            tracing::info!("Handled event: {}", envelope.event.message);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_publish_and_subscribe() {
        let bus = InMemoryEventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let handler = Arc::new(TestHandler {
            name: "test_handler",
            counter: counter.clone(),
        });

        let _subscription = bus.subscribe::<TestEvent, _>(handler).await.unwrap();

        // Give handler time to set up
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Publish event
        bus.publish(TestEvent {
            message: "Hello".to_string(),
        })
        .await
        .unwrap();

        // Give handler time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_multiple_handlers() {
        let bus = InMemoryEventBus::new();
        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));

        let handler1 = Arc::new(TestHandler {
            name: "handler_1",
            counter: counter1.clone(),
        });
        let handler2 = Arc::new(TestHandler {
            name: "handler_2",
            counter: counter2.clone(),
        });

        let _sub1 = bus.subscribe::<TestEvent, _>(handler1).await.unwrap();
        let _sub2 = bus.subscribe::<TestEvent, _>(handler2).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        bus.publish(TestEvent {
            message: "Broadcast".to_string(),
        })
        .await
        .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_subscription_cancel() {
        let bus = InMemoryEventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let handler = Arc::new(TestHandler {
            name: "cancellable",
            counter: counter.clone(),
        });

        let subscription = bus.subscribe::<TestEvent, _>(handler).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Cancel subscription
        subscription.cancel();

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Publish after cancel
        bus.publish(TestEvent {
            message: "After cancel".to_string(),
        })
        .await
        .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Should not have processed
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        assert!(!subscription.is_active());
    }

    #[tokio::test]
    async fn test_publish_without_subscribers() {
        let bus = InMemoryEventBus::new();

        // Should not error even with no subscribers
        let result = bus
            .publish(TestEvent {
                message: "No one listening".to_string(),
            })
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_subscription_count() {
        let bus = InMemoryEventBus::new();
        assert_eq!(bus.subscription_count(), 0);

        let counter = Arc::new(AtomicUsize::new(0));
        let handler = Arc::new(TestHandler {
            name: "counter_test",
            counter: counter.clone(),
        });

        let sub = bus.subscribe::<TestEvent, _>(handler).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        assert_eq!(bus.subscription_count(), 1);

        sub.cancel();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        assert_eq!(bus.subscription_count(), 0);
    }
}
</file>

<file path="crates/shared/src/lifecycle.rs">
// crates/shared/src/lifecycle.rs

use serde::{Serialize, Deserialize};
use time::OffsetDateTime;
use crate::hrn::Hrn;

/// Representa el estado del ciclo de vida de un Agregado, unificado y sin ambigüedad.
/// Es una máquina de estados simple: Active -> Archived -> Deleted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LifecycleState {
    /// El recurso está activo y operativo.
    Active,
    /// El recurso está archivado. Generalmente es de solo lectura y puede ser restaurado.
    Archived { at: OffsetDateTime, by: Hrn },
    /// El recurso ha sido marcado para borrado o borrado lógicamente. Es irrecuperable.
    Deleted { at: OffsetDateTime, by: Hrn },
}

/// Un Value Object que contiene información completa y consistente del ciclo de vida de un Agregado.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lifecycle {
    /// Fecha y hora de creación del recurso.
    pub created_at: OffsetDateTime,
    /// HRN del principal (User o ApiKey) que creó el recurso.
    pub created_by: Hrn,
    /// Fecha y hora de la última modificación del recurso.
    pub updated_at: OffsetDateTime,
    /// HRN del principal que realizó la última modificación.
    pub updated_by: Hrn,
    /// El estado actual del recurso (Activo, Archivado o Borrado).
    pub state: LifecycleState,
}

impl Lifecycle {
    pub fn new(creator_hrn: Hrn) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            created_at: now,
            created_by: creator_hrn.clone(),
            updated_at: now,
            updated_by: creator_hrn,
            state: LifecycleState::Active,
        }
    }
}
</file>

<file path="crates/shared/README.md">
# Shared Crate

Tipos, errores y utilidades compartidas para consistencia transversal en todo el workspace de Hodei Artifacts.

## Propósito

Este crate contiene:
- **Tipos de dominio comunes** (HRN, PackageCoordinates, etc.)
- **Enums compartidos** (ArtifactType, HashAlgorithm, etc.)
- **Estructuras de datos** (ContentHash, Lifecycle, etc.)
- **Utilidades de seguridad** (validación, autorización)
- **Modelos base** reutilizables entre crates

## Estructura

```
src/
  enums.rs           # Enums compartidos (ArtifactType, HashAlgorithm, etc.)
  hrn.rs             # Hodei Resource Names (identificadores únicos)
  lifecycle.rs       # Metadatos de auditoría (created_by, updated_at, etc.)
  models.rs          # Estructuras de datos comunes
  security/          # Utilidades de seguridad y autorización
    mod.rs
  lib.rs             # Re-exports públicos
```

## Tests

### Tests Unitarios

Los tests unitarios validan la lógica de construcción, validación y serialización de tipos compartidos:

```bash
# Ejecutar todos los tests unitarios del crate shared
cargo test --lib -p shared

# Ejecutar tests con logs detallados
RUST_LOG=debug cargo test --lib -p shared -- --nocapture

# Ejecutar tests de un módulo específico
cargo test -p shared hrn
cargo test -p shared models
cargo test -p shared enums
```

**Cobertura típica**:
- ✅ **Validación de HRN** - Formato correcto, parsing, construcción
- ✅ **Serialización JSON** - Serde para DTOs
- ✅ **Validación de tipos** - Enums, constraints de negocio
- ✅ **Lifecycle metadata** - Timestamps, user tracking

### Tests de Documentación

```bash
# Ejecutar doctests (ejemplos en comentarios ///)
cargo test --doc -p shared
```

## Desarrollo

### Agregar nuevos tipos compartidos

1. **Definir en el módulo apropiado** (`models.rs`, `enums.rs`, etc.)
2. **Añadir validación** si es necesario
3. **Incluir doctests** con ejemplos de uso
4. **Re-exportar** en `lib.rs` si es público
5. **Añadir tests unitarios** para casos edge

### Ejemplo de test unitario

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_coordinates_validation() {
        let coords = PackageCoordinates {
            namespace: Some("com.example".to_string()),
            name: "my-package".to_string(),
            version: "1.0.0".to_string(),
            qualifiers: Default::default(),
        };
        
        assert!(coords.is_valid());
        assert_eq!(coords.to_string(), "com.example:my-package:1.0.0");
    }
}
```

## Dependencies

- **Core**: `serde`, `time`, `uuid`, `thiserror`
- **Security**: `cedar-policy` (para ABAC)
- **Minimal external deps** para mantener el crate ligero

Ver `Cargo.toml` para versiones específicas.
</file>

<file path="crates/hodei-authorizer/src/features/evaluate_permissions/adapter.rs">
use async_trait::async_trait;
use cedar_policy::PolicySet;

use crate::features::evaluate_permissions::error::{
    EvaluatePermissionsError, EvaluatePermissionsResult,
};
use crate::features::evaluate_permissions::ports::{
    AuthorizationCache, AuthorizationLogger, AuthorizationMetrics, IamPolicyProvider,
    OrganizationBoundaryProvider,
};

/// Adapter implementation for IAM Policy Provider
pub struct IamPolicyProviderAdapter {
    // Implementation details here
}

#[async_trait]
impl IamPolicyProvider for IamPolicyProviderAdapter {
    async fn get_identity_policies_for(
        &self,
        _principal_hrn: &policies::shared::domain::hrn::Hrn,
    ) -> EvaluatePermissionsResult<PolicySet> {
        // TODO: Implement actual IAM policy retrieval
        Ok(PolicySet::new())
    }
}

/// Adapter implementation for Organization Boundary Provider
pub struct OrganizationBoundaryProviderAdapter {
    // Implementation details here
}

#[async_trait]
impl OrganizationBoundaryProvider for OrganizationBoundaryProviderAdapter {
    async fn get_effective_scps_for(
        &self,
        _entity_hrn: &policies::shared::domain::hrn::Hrn,
    ) -> EvaluatePermissionsResult<PolicySet> {
        // TODO: Implement actual SCP retrieval
        Ok(PolicySet::new())
    }
}

/// Adapter implementation for Authorization Cache
pub struct AuthorizationCacheAdapter {
    // Implementation details here
}

#[async_trait]
impl AuthorizationCache for AuthorizationCacheAdapter {
    async fn get(
        &self,
        _cache_key: &str,
    ) -> EvaluatePermissionsResult<
        Option<crate::features::evaluate_permissions::dto::AuthorizationResponse>,
    > {
        // TODO: Implement actual cache retrieval
        Ok(None)
    }

    async fn put(
        &self,
        _cache_key: &str,
        _response: &crate::features::evaluate_permissions::dto::AuthorizationResponse,
        _ttl: std::time::Duration,
    ) -> EvaluatePermissionsResult<()> {
        // TODO: Implement actual cache storage
        Ok(())
    }

    async fn invalidate_principal(
        &self,
        _principal_hrn: &policies::shared::domain::hrn::Hrn,
    ) -> EvaluatePermissionsResult<()> {
        // TODO: Implement principal cache invalidation
        Ok(())
    }

    async fn invalidate_resource(
        &self,
        _resource_hrn: &policies::shared::domain::hrn::Hrn,
    ) -> EvaluatePermissionsResult<()> {
        // TODO: Implement resource cache invalidation
        Ok(())
    }
}

/// Adapter implementation for Authorization Logger
pub struct AuthorizationLoggerAdapter {
    // Implementation details here
}

#[async_trait]
impl AuthorizationLogger for AuthorizationLoggerAdapter {
    async fn log_decision(
        &self,
        request: &crate::features::evaluate_permissions::dto::AuthorizationRequest,
        response: &crate::features::evaluate_permissions::dto::AuthorizationResponse,
    ) -> EvaluatePermissionsResult<()> {
        // TODO: Implement actual logging
        tracing::info!(
            "Authorization decision: {:?} for request: {:?}",
            response.decision,
            request
        );
        Ok(())
    }

    async fn log_error(
        &self,
        request: &crate::features::evaluate_permissions::dto::AuthorizationRequest,
        error: &EvaluatePermissionsError,
    ) -> EvaluatePermissionsResult<()> {
        // TODO: Implement actual error logging
        tracing::error!(
            "Authorization error: {:?} for request: {:?}",
            error,
            request
        );
        Ok(())
    }
}

/// Adapter implementation for Authorization Metrics
pub struct AuthorizationMetricsAdapter {
    // Implementation details here
}

#[async_trait]
impl AuthorizationMetrics for AuthorizationMetricsAdapter {
    async fn record_decision(
        &self,
        decision: &crate::features::evaluate_permissions::dto::AuthorizationDecision,
        evaluation_time_ms: u64,
    ) -> EvaluatePermissionsResult<()> {
        // TODO: Implement actual metrics recording
        tracing::info!(
            "Recorded decision: {:?} in {}ms",
            decision,
            evaluation_time_ms
        );
        Ok(())
    }

    async fn record_error(&self, error_type: &str) -> EvaluatePermissionsResult<()> {
        // TODO: Implement actual error metrics recording
        tracing::info!("Recorded error type: {}", error_type);
        Ok(())
    }

    async fn record_cache_hit(&self, hit: bool) -> EvaluatePermissionsResult<()> {
        // TODO: Implement actual cache hit metrics recording
        tracing::info!("Cache hit: {}", hit);
        Ok(())
    }
}
</file>

<file path="crates/hodei-authorizer/src/features/evaluate_permissions/mod.rs">
//! Feature for evaluating authorization permissions with multi-layer security
//!
//! This feature provides comprehensive authorization evaluation combining:
//! - IAM policies (user and group permissions)
//! - Service Control Policies (SCP) for organizational boundaries
//! - Cedar policy engine for evaluation
//! - Caching, logging, and metrics

pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
pub mod mocks;
pub mod ports;
pub mod use_case;

// Re-export main types for easier access
pub use dto::{
    AuthorizationContext, AuthorizationDecision, AuthorizationRequest, AuthorizationResponse,
    PolicyImpact,
};

pub use error::{EvaluatePermissionsError, EvaluatePermissionsResult};

pub use ports::{AuthorizationCache, AuthorizationLogger, AuthorizationMetrics};

pub use use_case::EvaluatePermissionsUseCase;

pub use di::{EvaluatePermissionsContainer, EvaluatePermissionsContainerBuilder, factories};

// Re-export mocks for testing
#[cfg(test)]
pub use mocks::{
    MockAuthorizationCache, MockAuthorizationLogger, MockAuthorizationMetrics, test_helpers,
};

/// Feature version and metadata
pub const FEATURE_VERSION: &str = "1.0.0";
pub const FEATURE_NAME: &str = "evaluate_permissions";

/// Configuration for the evaluate permissions feature
#[derive(Debug, Clone)]
pub struct EvaluatePermissionsConfig {
    /// Cache TTL in seconds (default: 300 = 5 minutes)
    pub cache_ttl_secs: u64,
    /// Enable/disable caching
    pub cache_enabled: bool,
    /// Enable/disable detailed logging
    pub detailed_logging: bool,
    /// Enable/disable metrics collection
    pub metrics_enabled: bool,
    /// Maximum evaluation time in milliseconds
    pub max_evaluation_time_ms: u64,
}

impl Default for EvaluatePermissionsConfig {
    fn default() -> Self {
        Self {
            cache_ttl_secs: 300,
            cache_enabled: true,
            detailed_logging: true,
            metrics_enabled: true,
            max_evaluation_time_ms: 5000, // 5 seconds
        }
    }
}

impl EvaluatePermissionsConfig {
    /// Create a new configuration with custom settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Set cache TTL
    pub fn with_cache_ttl(mut self, ttl_secs: u64) -> Self {
        self.cache_ttl_secs = ttl_secs;
        self
    }

    /// Enable/disable cache
    pub fn with_cache_enabled(mut self, enabled: bool) -> Self {
        self.cache_enabled = enabled;
        self
    }

    /// Enable/disable detailed logging
    pub fn with_detailed_logging(mut self, enabled: bool) -> Self {
        self.detailed_logging = enabled;
        self
    }

    /// Enable/disable metrics
    pub fn with_metrics_enabled(mut self, enabled: bool) -> Self {
        self.metrics_enabled = enabled;
        self
    }

    /// Set maximum evaluation time
    pub fn with_max_evaluation_time(mut self, time_ms: u64) -> Self {
        self.max_evaluation_time_ms = time_ms;
        self
    }
}

/// Utility functions for the evaluate permissions feature
pub mod utils {
    use super::*;
    use std::time::Duration;

    /// Convert configuration TTL to Duration
    pub fn ttl_to_duration(config: &EvaluatePermissionsConfig) -> Duration {
        Duration::from_secs(config.cache_ttl_secs)
    }

    /// Generate a cache key for authorization requests
    pub fn generate_cache_key(request: &AuthorizationRequest) -> String {
        format!(
            "auth:{}:{}:{}",
            request.principal, request.action, request.resource
        )
    }

    /// Validate authorization request
    pub fn validate_request(
        request: &AuthorizationRequest,
    ) -> Result<(), EvaluatePermissionsError> {
        if request.action.is_empty() {
            return Err(EvaluatePermissionsError::InvalidRequest(
                "Action cannot be empty".to_string(),
            ));
        }

        if request.principal.to_string().is_empty() {
            return Err(EvaluatePermissionsError::InvalidRequest(
                "Principal cannot be empty".to_string(),
            ));
        }

        if request.resource.to_string().is_empty() {
            return Err(EvaluatePermissionsError::InvalidRequest(
                "Resource cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    /// Create a default authorization context if none provided
    pub fn ensure_context(request: &mut AuthorizationRequest) {
        if request.context.is_none() {
            request.context = Some(AuthorizationContext::default());
        }
    }
}

#[cfg(test)]
mod feature_tests {
    use super::mocks::test_helpers;
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_feature_version() {
        assert_eq!(FEATURE_VERSION, "1.0.0");
        assert_eq!(FEATURE_NAME, "evaluate_permissions");
    }

    #[test]
    fn test_config_default() {
        let config = EvaluatePermissionsConfig::default();
        assert_eq!(config.cache_ttl_secs, 300);
        assert!(config.cache_enabled);
        assert!(config.detailed_logging);
        assert!(config.metrics_enabled);
        assert_eq!(config.max_evaluation_time_ms, 5000);
    }

    #[test]
    fn test_config_builder() {
        let config = EvaluatePermissionsConfig::new()
            .with_cache_ttl(600)
            .with_cache_enabled(false)
            .with_detailed_logging(false)
            .with_metrics_enabled(false)
            .with_max_evaluation_time(10000);

        assert_eq!(config.cache_ttl_secs, 600);
        assert!(!config.cache_enabled);
        assert!(!config.detailed_logging);
        assert!(!config.metrics_enabled);
        assert_eq!(config.max_evaluation_time_ms, 10000);
    }

    #[test]
    fn test_utils_generate_cache_key() {
        let principal = test_helpers::create_test_hrn("user", "alice");
        let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
        let request = AuthorizationRequest::new(principal, "read".to_string(), resource);

        let cache_key = utils::generate_cache_key(&request);
        assert!(cache_key.contains("auth:"));
        assert!(cache_key.contains("read"));
    }

    #[test]
    fn test_utils_validate_request() {
        let principal = test_helpers::create_test_hrn("user", "alice");
        let resource = test_helpers::create_test_hrn("bucket", "test-bucket");

        // Valid request
        let valid_request =
            AuthorizationRequest::new(principal.clone(), "read".to_string(), resource.clone());
        assert!(utils::validate_request(&valid_request).is_ok());

        // Invalid request - empty action
        let invalid_request =
            AuthorizationRequest::new(principal.clone(), "".to_string(), resource.clone());
        assert!(utils::validate_request(&invalid_request).is_err());

        // Note: Hrn always produces a valid string representation,
        // so we skip the "empty principal" test as it's not realistic
        // with the current HRN implementation
    }

    #[test]
    fn test_utils_ensure_context() {
        let principal = test_helpers::create_test_hrn("user", "alice");
        let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
        let mut request = AuthorizationRequest::new(principal, "read".to_string(), resource);

        assert!(request.context.is_none());
        utils::ensure_context(&mut request);
        assert!(request.context.is_some());
    }

    #[test]
    fn test_utils_ttl_to_duration() {
        let config = EvaluatePermissionsConfig::new().with_cache_ttl(120);
        let duration = utils::ttl_to_duration(&config);
        assert_eq!(duration, Duration::from_secs(120));
    }
}
</file>

<file path="crates/hodei-authorizer/src/features/evaluate_permissions/ports.rs">
use async_trait::async_trait;
use cedar_policy::PolicySet;
use std::sync::Arc;

use crate::features::evaluate_permissions::dto::{AuthorizationRequest, AuthorizationResponse};
use crate::features::evaluate_permissions::error::EvaluatePermissionsResult;
use policies::shared::domain::hrn::Hrn;

/// Trait for providing IAM policies
#[async_trait]
pub trait IamPolicyProvider: Send + Sync {
    async fn get_identity_policies_for(
        &self,
        principal_hrn: &Hrn,
    ) -> EvaluatePermissionsResult<PolicySet>;
}

#[async_trait]
impl<T: IamPolicyProvider> IamPolicyProvider for Arc<T> {
    async fn get_identity_policies_for(
        &self,
        principal_hrn: &Hrn,
    ) -> EvaluatePermissionsResult<PolicySet> {
        (**self).get_identity_policies_for(principal_hrn).await
    }
}

/// Trait for providing organization boundary policies (SCPs)
#[async_trait]
pub trait OrganizationBoundaryProvider: Send + Sync {
    async fn get_effective_scps_for(
        &self,
        entity_hrn: &Hrn,
    ) -> EvaluatePermissionsResult<PolicySet>;
}

#[async_trait]
impl<T: OrganizationBoundaryProvider> OrganizationBoundaryProvider for Arc<T> {
    async fn get_effective_scps_for(
        &self,
        entity_hrn: &Hrn,
    ) -> EvaluatePermissionsResult<PolicySet> {
        (**self).get_effective_scps_for(entity_hrn).await
    }
}

/// Trait for caching authorization decisions
#[async_trait]
pub trait AuthorizationCache: Send + Sync {
    async fn get(
        &self,
        cache_key: &str,
    ) -> EvaluatePermissionsResult<Option<AuthorizationResponse>>;
    async fn put(
        &self,
        cache_key: &str,
        response: &AuthorizationResponse,
        ttl: std::time::Duration,
    ) -> EvaluatePermissionsResult<()>;
    async fn invalidate_principal(&self, principal_hrn: &Hrn) -> EvaluatePermissionsResult<()>;
    async fn invalidate_resource(&self, resource_hrn: &Hrn) -> EvaluatePermissionsResult<()>;
}

#[async_trait]
impl<T: AuthorizationCache> AuthorizationCache for Arc<T> {
    async fn get(
        &self,
        cache_key: &str,
    ) -> EvaluatePermissionsResult<Option<AuthorizationResponse>> {
        (**self).get(cache_key).await
    }

    async fn put(
        &self,
        cache_key: &str,
        response: &AuthorizationResponse,
        ttl: std::time::Duration,
    ) -> EvaluatePermissionsResult<()> {
        (**self).put(cache_key, response, ttl).await
    }

    async fn invalidate_principal(&self, principal_hrn: &Hrn) -> EvaluatePermissionsResult<()> {
        (**self).invalidate_principal(principal_hrn).await
    }

    async fn invalidate_resource(&self, resource_hrn: &Hrn) -> EvaluatePermissionsResult<()> {
        (**self).invalidate_resource(resource_hrn).await
    }
}

/// Trait for logging authorization decisions and errors
#[async_trait]
pub trait AuthorizationLogger: Send + Sync {
    async fn log_decision(
        &self,
        request: &AuthorizationRequest,
        response: &AuthorizationResponse,
    ) -> EvaluatePermissionsResult<()>;
    async fn log_error(
        &self,
        request: &AuthorizationRequest,
        error: &super::error::EvaluatePermissionsError,
    ) -> EvaluatePermissionsResult<()>;
}

#[async_trait]
impl<T: AuthorizationLogger> AuthorizationLogger for Arc<T> {
    async fn log_decision(
        &self,
        request: &AuthorizationRequest,
        response: &AuthorizationResponse,
    ) -> EvaluatePermissionsResult<()> {
        (**self).log_decision(request, response).await
    }

    async fn log_error(
        &self,
        request: &AuthorizationRequest,
        error: &super::error::EvaluatePermissionsError,
    ) -> EvaluatePermissionsResult<()> {
        (**self).log_error(request, error).await
    }
}

/// Trait for recording authorization metrics
#[async_trait]
pub trait AuthorizationMetrics: Send + Sync {
    async fn record_decision(
        &self,
        decision: &super::dto::AuthorizationDecision,
        evaluation_time_ms: u64,
    ) -> EvaluatePermissionsResult<()>;
    async fn record_error(&self, error_type: &str) -> EvaluatePermissionsResult<()>;
    async fn record_cache_hit(&self, hit: bool) -> EvaluatePermissionsResult<()>;
}

#[async_trait]
impl<T: AuthorizationMetrics> AuthorizationMetrics for Arc<T> {
    async fn record_decision(
        &self,
        decision: &super::dto::AuthorizationDecision,
        evaluation_time_ms: u64,
    ) -> EvaluatePermissionsResult<()> {
        (**self).record_decision(decision, evaluation_time_ms).await
    }

    async fn record_error(&self, error_type: &str) -> EvaluatePermissionsResult<()> {
        (**self).record_error(error_type).await
    }

    async fn record_cache_hit(&self, hit: bool) -> EvaluatePermissionsResult<()> {
        (**self).record_cache_hit(hit).await
    }
}

/// Errors related to authorization ports
#[derive(Debug, thiserror::Error)]
pub enum AuthorizationError {
    #[error("IAM policy provider error: {0}")]
    IamPolicyProvider(String),
    #[error("Organization boundary provider error: {0}")]
    OrganizationBoundaryProvider(String),
}
</file>

<file path="crates/hodei-authorizer/src/lib.rs">
pub mod application;
pub mod contracts;
pub mod dto;
pub mod features;

// Re-export evaluate permissions feature
pub use features::evaluate_permissions::{
    di::{EvaluatePermissionsContainer, EvaluatePermissionsContainerBuilder},
    dto::{
        AuthorizationDecision as EvalAuthDecision, AuthorizationRequest as EvalAuthRequest,
        AuthorizationResponse,
    },
    error::EvaluatePermissionsError,
    ports::{
        IamPolicyProvider as EvalIamPolicyProvider, OrganizationBoundaryProvider as EvalOrgProvider,
    },
    use_case::EvaluatePermissionsUseCase,
};
</file>

<file path="crates/hodei-authorizer/Cargo.toml">
[package]
name = "hodei-authorizer"
version = "0.1.0"
edition = "2024"

[dependencies]
policies = { path = "../policies" }
hodei-organizations = { path = "../hodei-organizations" }
hodei-iam = { path = "../hodei-iam" }
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "1.0"
async-trait = "0.1"
cedar-policy = { workspace = true }
tracing = "0.1"
time = { version = "0.3", features = ["serde", "serde-well-known"] }
</file>

<file path="crates/hodei-iam/src/features/add_user_to_group/dto.rs">
/// Data Transfer Objects for add_user_to_group feature

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddUserToGroupCommand {
    pub user_hrn: String,
    pub group_hrn: String,
}
</file>

<file path="crates/hodei-iam/src/features/create_group/dto.rs">
/// Data Transfer Objects for create_group feature

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGroupCommand {
    pub group_name: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupView {
    pub hrn: String,
    pub name: String,
    pub tags: Vec<String>,
}
</file>

<file path="crates/hodei-iam/src/features/create_user/dto.rs">
/// Data Transfer Objects for create_user feature

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserCommand {
    pub name: String,
    pub email: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserView {
    pub hrn: String,
    pub name: String,
    pub email: String,
    pub groups: Vec<String>,
    pub tags: Vec<String>,
}
</file>

<file path="crates/hodei-iam/src/features/get_effective_policies_for_principal/mod.rs">
//! Feature: Get Effective Policies for Principal
//!
//! Este caso de uso proporciona la ÚNICA forma de que otros crates obtengan
//! las políticas IAM efectivas para un principal.
//!
//! # Contrato Público
//! - Input: `GetEffectivePoliciesQuery` (HRN del principal)
//! - Output: `EffectivePoliciesResponse` (PolicySet de Cedar)
//!
//! # Encapsulación
//! Las entidades internas (User, Group, Policy) NO se exponen.
//! Solo se devuelve un PolicySet de Cedar que puede ser usado directamente
//! por el motor de autorización.

pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;

// Re-exports públicos para acceso externo
pub use adapter::{GroupFinderAdapter, PolicyFinderAdapter, UserFinderAdapter};
pub use di::make_use_case;
pub use dto::{EffectivePoliciesResponse, GetEffectivePoliciesQuery};
pub use error::{GetEffectivePoliciesError, GetEffectivePoliciesResult};
pub use ports::{GroupFinderPort, PolicyFinderPort, UserFinderPort};
pub use use_case::GetEffectivePoliciesForPrincipalUseCase;
</file>

<file path="crates/hodei-iam/src/features/get_effective_policies_for_principal/use_case.rs">
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
use policies::shared::domain::hrn::Hrn;
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
    use crate::shared::domain::{Group, User};

    // Mock implementations for testing
    struct MockUserFinder {
        users: Vec<User>,
    }

    #[async_trait::async_trait]
    impl UserFinderPort for MockUserFinder {
        async fn find_by_hrn(
            &self,
            hrn: &Hrn,
        ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>> {
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
        ) -> Result<Vec<Group>, Box<dyn std::error::Error + Send + Sync>> {
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
        ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
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
        // Setup
        let user_hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/test-user").unwrap();
        let user = User::new(
            user_hrn.clone(),
            "test-user".to_string(),
            "test@example.com".to_string(),
        );

        let group_hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:group/admins").unwrap();
        let group = Group::new(group_hrn.clone(), "admins".to_string());

        let user_policy = r#"permit(principal, action, resource);"#.to_string();
        let group_policy = r#"permit(principal, action == Action::"read", resource);"#.to_string();

        let user_finder = Arc::new(MockUserFinder {
            users: vec![user.clone()],
        });

        let group_finder = Arc::new(MockGroupFinder {
            groups: vec![(user_hrn.clone(), vec![group.clone()])],
        });

        let policy_finder = Arc::new(MockPolicyFinder {
            policies: vec![
                (user_hrn.clone(), vec![user_policy]),
                (group_hrn.clone(), vec![group_policy]),
            ],
        });

        let use_case =
            GetEffectivePoliciesForPrincipalUseCase::new(user_finder, group_finder, policy_finder);

        // Execute
        let query = GetEffectivePoliciesQuery {
            principal_hrn: "hrn:hodei:iam:us-east-1:default:user/test-user".to_string(),
        };

        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(
            response.principal_hrn,
            "hrn:hodei:iam:us-east-1:default:user/test-user"
        );
        assert_eq!(response.policy_count, 2);
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
</file>

<file path="crates/hodei-iam/src/features/mod.rs">
/// Features module for hodei-iam
///
/// This module contains all the use cases (features) organized as vertical slices
pub mod add_user_to_group;
pub mod create_group;
pub mod create_user;
pub mod get_effective_policies_for_principal;
</file>

<file path="crates/hodei-iam/src/shared/application/ports/mod.rs">
use crate::shared::domain::{Group, User};
/// Application ports (interfaces) for hodei-iam
///
/// This module defines the traits (ports) that the application layer uses
/// to interact with infrastructure concerns like persistence.

use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn save(&self, user: &User) -> Result<(), anyhow::Error>;
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, anyhow::Error>;
    async fn find_all(&self) -> Result<Vec<User>, anyhow::Error>;
}

#[async_trait]
pub trait GroupRepository: Send + Sync {
    async fn save(&self, group: &Group) -> Result<(), anyhow::Error>;
    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, anyhow::Error>;
    async fn find_all(&self) -> Result<Vec<Group>, anyhow::Error>;
}
</file>

<file path="crates/hodei-iam/src/shared/application/di_configurator.rs">
use crate::shared::domain::{CreateGroupAction, CreateUserAction, Group, Namespace, ServiceAccount, User};
/// DI Configurator for hodei-iam
/// 
/// Provides a function to configure the policies EngineBuilder with default IAM entities

use anyhow::Result;
use policies::shared::application::EngineBuilder;

/// Configure an EngineBuilder with default IAM entities
/// 
/// This function registers:
/// - Principals: User, ServiceAccount
/// - Resources: User, Group, ServiceAccount, Namespace
/// - Actions: CreateUserAction, CreateGroupAction
/// 
/// # Example
/// ```no_run
/// use policies::shared::application::di_helpers;
/// use hodei_iam::shared::application::configure_default_iam_entities;
/// 
/// # async fn example() -> anyhow::Result<()> {
/// let (engine, store) = di_helpers::build_engine_mem(configure_default_iam_entities).await?;
/// # Ok(())
/// # }
/// ```
pub fn configure_default_iam_entities(mut builder: EngineBuilder) -> Result<EngineBuilder> {
    builder
        .register_principal::<User>()?
        .register_principal::<ServiceAccount>()?
        .register_resource::<User>()?
        .register_resource::<Group>()?
        .register_resource::<ServiceAccount>()?
        .register_resource::<Namespace>()?
        .register_action::<CreateUserAction>()?
        .register_action::<CreateGroupAction>()?;
    Ok(builder)
}
</file>

<file path="crates/hodei-iam/src/shared/domain/actions.rs">
use cedar_policy::EntityTypeName;
/// Domain actions for hodei-iam
/// 
/// This module defines the IAM actions that can be performed

use policies::shared::domain::ports::Action;
use std::str::FromStr;

pub struct CreateUserAction;

impl Action for CreateUserAction {
    fn name() -> &'static str {
        "create_user"
    }
    
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        (
            EntityTypeName::from_str("User").expect("Valid entity type"),
            EntityTypeName::from_str("User").expect("Valid entity type"),
        )
    }
}

pub struct CreateGroupAction;

impl Action for CreateGroupAction {
    fn name() -> &'static str {
        "create_group"
    }
    
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        (
            EntityTypeName::from_str("User").expect("Valid entity type"),
            EntityTypeName::from_str("Group").expect("Valid entity type"),
        )
    }
}
</file>

<file path="crates/hodei-iam/src/shared/domain/entities.rs">
use cedar_policy::{EntityUid, RestrictedExpression};
/// Domain entities for hodei-iam
/// 
/// This module defines the core IAM entities: User, Group, ServiceAccount, Namespace

use policies::shared::domain::hrn::Hrn;
use policies::shared::domain::ports::{self, HodeiEntity, HodeiEntityType, Principal, Resource};
use ports::AttributeType::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub hrn: Hrn,
    pub name: String,
    pub group_hrns: Vec<Hrn>,
    pub email: String,
    pub tags: Vec<String>,
}

impl User {
    /// Create a new User
    pub fn new(hrn: Hrn, name: String, email: String) -> Self {
        Self {
            hrn,
            name,
            email,
            group_hrns: Vec::new(),
            tags: Vec::new(),
        }
    }

    /// Add user to a group (idempotent - won't add duplicates)
    pub fn add_to_group(&mut self, group_hrn: Hrn) {
        if !self.group_hrns.contains(&group_hrn) {
            self.group_hrns.push(group_hrn);
        }
    }

    /// Remove user from a group
    pub fn remove_from_group(&mut self, group_hrn: &Hrn) {
        self.group_hrns.retain(|hrn| hrn != group_hrn);
    }

    /// Get all groups this user belongs to
    pub fn groups(&self) -> &[Hrn] {
        &self.group_hrns
    }

    /// Get user's email
    pub fn email(&self) -> &str {
        &self.email
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub hrn: Hrn,
    pub name: String,
    pub tags: Vec<String>,
    pub attached_policy_hrns: Vec<Hrn>,
}

impl Group {
    /// Create a new Group
    pub fn new(hrn: Hrn, name: String) -> Self {
        Self {
            hrn,
            name,
            tags: Vec::new(),
            attached_policy_hrns: Vec::new(),
        }
    }

    /// Attach a policy to this group (idempotent)
    pub fn attach_policy(&mut self, policy_hrn: Hrn) {
        if !self.attached_policy_hrns.contains(&policy_hrn) {
            self.attached_policy_hrns.push(policy_hrn);
        }
    }

    /// Detach a policy from this group
    pub fn detach_policy(&mut self, policy_hrn: &Hrn) {
        self.attached_policy_hrns.retain(|hrn| hrn != policy_hrn);
    }

    /// Get group name
    pub fn group_name(&self) -> &str {
        &self.name
    }

    /// Get attached policies
    pub fn attached_policies(&self) -> &[Hrn] {
        &self.attached_policy_hrns
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccount {
    pub hrn: Hrn,
    pub name: String,
    pub annotations: HashMap<String, String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespace {
    pub hrn: Hrn,
    pub name: String,
    pub tags: Vec<String>,
    pub annotations: HashMap<String, String>,
}

// --- Implementaciones para User ---

impl HodeiEntityType for User {
    fn service_name() -> &'static str {
        "iam"
    }

    fn resource_type_name() -> &'static str {
        "User"
    }

    fn is_principal_type() -> bool {
        true
    }

    fn cedar_attributes() -> Vec<(&'static str, ports::AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("email", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

impl HodeiEntity for User {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn parents(&self) -> Vec<EntityUid> {
        self.group_hrns.iter().map(|hrn| hrn.euid()).collect()
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );
        attrs.insert(
            "email".to_string(),
            RestrictedExpression::new_string(self.email.clone()),
        );
        let tag_exprs: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }
}

impl Principal for User {}
impl Resource for User {}

// --- Implementaciones para Group ---

impl HodeiEntityType for Group {
    fn service_name() -> &'static str {
        "iam"
    }

    fn resource_type_name() -> &'static str {
        "Group"
    }

    fn cedar_attributes() -> Vec<(&'static str, ports::AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

impl HodeiEntity for Group {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );
        let tag_exprs: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Resource for Group {}

// --- Implementaciones para ServiceAccount ---

impl HodeiEntityType for ServiceAccount {
    fn service_name() -> &'static str {
        "iam"
    }

    fn resource_type_name() -> &'static str {
        "ServiceAccount"
    }

    fn is_principal_type() -> bool {
        true
    }

    fn cedar_attributes() -> Vec<(&'static str, ports::AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("annotations", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

impl HodeiEntity for ServiceAccount {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );

        let annotation_map: BTreeMap<String, RestrictedExpression> = self
            .annotations
            .iter()
            .map(|(k, v)| (k.clone(), RestrictedExpression::new_string(v.clone())))
            .collect();
        attrs.insert(
            "annotations".to_string(),
            RestrictedExpression::new_record(annotation_map).unwrap_or_else(|_| {
                RestrictedExpression::new_string("error_creating_record".to_string())
            }),
        );

        let tag_exprs: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Principal for ServiceAccount {}
impl Resource for ServiceAccount {}

// --- Implementaciones para Namespace ---

impl HodeiEntityType for Namespace {
    fn service_name() -> &'static str { "iam" }
    fn resource_type_name() -> &'static str { "Namespace" }

    fn cedar_attributes() -> Vec<(&'static str, ports::AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("annotations", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

impl HodeiEntity for Namespace {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );

        let annotation_map: BTreeMap<String, RestrictedExpression> = self
            .annotations
            .iter()
            .map(|(k, v)| (k.clone(), RestrictedExpression::new_string(v.clone())))
            .collect();
        attrs.insert(
            "annotations".to_string(),
            RestrictedExpression::new_record(annotation_map).unwrap_or_else(|_| {
                RestrictedExpression::new_string("error_creating_record".to_string())
            }),
        );

        let tag_exprs: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Resource for Namespace {}
</file>

<file path="crates/hodei-iam/src/shared/infrastructure/persistence/mod.rs">
use crate::shared::application::ports::{GroupRepository, UserRepository};
use crate::shared::domain::{Group, User};
/// In-memory repository implementations for testing
///
/// These repositories store data in memory using RwLock for thread-safe access

use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;
use std::sync::RwLock;

/// In-memory implementation of UserRepository for testing
#[derive(Debug, Default)]
pub struct InMemoryUserRepository {
    users: RwLock<Vec<User>>,
}

impl InMemoryUserRepository {
    pub fn new() -> Self {
        Self {
            users: RwLock::new(Vec::new()),
        }
    }
}

#[async_trait]
impl UserRepository for InMemoryUserRepository {
    async fn save(&self, user: &User) -> Result<(), anyhow::Error> {
        let mut users = self.users.write().unwrap();

        // Remove existing user with same HRN if present
        users.retain(|u| u.hrn != user.hrn);

        // Add the new/updated user
        users.push(user.clone());

        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<User>, anyhow::Error> {
        let users = self.users.read().unwrap();
        Ok(users.iter().find(|u| &u.hrn == hrn).cloned())
    }

    async fn find_all(&self) -> Result<Vec<User>, anyhow::Error> {
        let users = self.users.read().unwrap();
        Ok(users.clone())
    }
}

/// In-memory implementation of GroupRepository for testing
#[derive(Debug, Default)]
pub struct InMemoryGroupRepository {
    groups: RwLock<Vec<Group>>,
}

impl InMemoryGroupRepository {
    pub fn new() -> Self {
        Self {
            groups: RwLock::new(Vec::new()),
        }
    }
}

#[async_trait]
impl GroupRepository for InMemoryGroupRepository {
    async fn save(&self, group: &Group) -> Result<(), anyhow::Error> {
        let mut groups = self.groups.write().unwrap();

        // Remove existing group with same HRN if present
        groups.retain(|g| g.hrn != group.hrn);

        // Add the new/updated group
        groups.push(group.clone());

        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Group>, anyhow::Error> {
        let groups = self.groups.read().unwrap();
        Ok(groups.iter().find(|g| &g.hrn == hrn).cloned())
    }

    async fn find_all(&self) -> Result<Vec<Group>, anyhow::Error> {
        let groups = self.groups.read().unwrap();
        Ok(groups.clone())
    }
}
</file>

<file path="crates/hodei-iam/src/shared/mod.rs">
//! Shared kernel for hodei-iam
pub mod application;
pub mod domain;
pub mod infrastructure;
</file>

<file path="crates/hodei-iam/tests/add_user_to_group_integration_test.rs">
/// Integration tests for add_user_to_group feature
///
/// These tests use in-memory repositories and coordinate between two aggregates

use hodei_iam::{
    features::{
        add_user_to_group::{self, dto::AddUserToGroupCommand},
        create_group::{self as create_group_feature, dto::CreateGroupCommand},
        create_user::{self as create_user_feature, dto::CreateUserCommand},
    },
    shared::{
        application::ports::UserRepository,
        infrastructure::persistence::{InMemoryGroupRepository, InMemoryUserRepository},
    },
};
use std::sync::Arc;


#[tokio::test]
async fn test_add_user_to_group_success() {
    // Arrange
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a user
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let user_cmd = CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec![],
    };
    let user_view = create_user_uc.execute(user_cmd).await.unwrap();

    // Create a group
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let group_cmd = CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    };
    let group_view = create_group_uc.execute(group_cmd).await.unwrap();

    // Act - Add user to group
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());
    let add_cmd = AddUserToGroupCommand {
        user_hrn: user_view.hrn.clone(),
        group_hrn: group_view.hrn.clone(),
    };
    let result = add_uc.execute(add_cmd).await;

    // Assert
    assert!(result.is_ok());

    // Verify that the user now belongs to the group
    let users = user_repo.find_all().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].groups().len(), 1);
    assert_eq!(users[0].groups()[0].to_string(), group_view.hrn);
}

#[tokio::test]
async fn test_add_user_to_group_idempotent() {
    // Arrange
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a user and group
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc.execute(CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc.execute(CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Act - Add user to group twice
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());
    let add_cmd = AddUserToGroupCommand {
        user_hrn: user_view.hrn.clone(),
        group_hrn: group_view.hrn.clone(),
    };

    let result1 = add_uc.execute(add_cmd.clone()).await;
    let result2 = add_uc.execute(add_cmd).await;

    // Assert - Both operations succeed
    assert!(result1.is_ok());
    assert!(result2.is_ok());

    // Verify that the user only has one group (no duplicates)
    let users = user_repo.find_all().await.unwrap();
    assert_eq!(users[0].groups().len(), 1);
}

#[tokio::test]
async fn test_add_user_to_nonexistent_group_fails() {
    // Arrange
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create only a user (no group)
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc.execute(CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Act - Try to add user to nonexistent group
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());
    let add_cmd = AddUserToGroupCommand {
        user_hrn: user_view.hrn,
        group_hrn: "hrn:hodei:iam:default:Group:nonexistent".to_string(),
    };
    let result = add_uc.execute(add_cmd).await;

    // Assert - Operation fails
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    println!("Error message: {}", err_msg);
    assert!(err_msg.contains("Invalid group HRN") || err_msg.contains("Group not found"));
}

#[tokio::test]
async fn test_add_nonexistent_user_to_group_fails() {
    // Arrange
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create only a group (no user)
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc.execute(CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Act - Try to add nonexistent user to group
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());
    let add_cmd = AddUserToGroupCommand {
        user_hrn: "hrn:hodei:iam:default:User:nonexistent".to_string(),
        group_hrn: group_view.hrn,
    };
    let result = add_uc.execute(add_cmd).await;

    // Assert - Operation fails
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    println!("Error message: {}", err_msg);
    assert!(err_msg.contains("Invalid user HRN") || err_msg.contains("User not found"));
}
</file>

<file path="crates/hodei-iam/tests/create_user_integration_test.rs">
/// Integration tests for create_user feature
///
/// These tests use in-memory repositories to validate the complete vertical slice

use hodei_iam::{
    features::create_user::{self, dto::*},
    shared::{
        application::ports::UserRepository,
        infrastructure::persistence::InMemoryUserRepository,
    },
};
use std::sync::Arc;


#[tokio::test]
async fn test_create_user_success() {
    // Arrange
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "Alice");
    assert_eq!(view.email, "alice@example.com");
    assert_eq!(view.groups.len(), 0); // No groups initially
    assert_eq!(view.tags.len(), 1);
    assert!(view.hrn.contains("User"));

    // Verify persistence
    let users = repo.find_all().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].name, "Alice");
    assert_eq!(users[0].email, "alice@example.com");
}

#[tokio::test]
async fn test_create_multiple_users() {
    // Arrange
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    // Act - Create multiple users
    let cmd1 = CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };
    let cmd2 = CreateUserCommand {
        name: "Bob".to_string(),
        email: "bob@example.com".to_string(),
        tags: vec!["developer".to_string()],
    };

    let result1 = use_case.execute(cmd1).await;
    let result2 = use_case.execute(cmd2).await;

    // Assert
    assert!(result1.is_ok());
    assert!(result2.is_ok());

    let users = repo.find_all().await.unwrap();
    assert_eq!(users.len(), 2);
}
</file>

<file path="crates/hodei-iam/tests/integration_add_user_to_group_comprehensive_test.rs">
/// Comprehensive integration tests for add_user_to_group feature

use hodei_iam::{
    features::{
        add_user_to_group::{self, dto::AddUserToGroupCommand},
        create_group::{self as create_group_feature, dto::CreateGroupCommand},
        create_user::{self as create_user_feature, dto::CreateUserCommand},
    },
    shared::{
        application::ports::UserRepository,
        infrastructure::persistence::{InMemoryGroupRepository, InMemoryUserRepository},
    },
};
use std::sync::Arc;


#[tokio::test]
async fn test_add_multiple_users_to_same_group() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a group
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let group_view = create_group_uc.execute(CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Create multiple users and add them to the group
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());

    let users = vec!["Alice", "Bob", "Charlie"];

    for user_name in users {
        let user_view = create_user_uc.execute(CreateUserCommand {
            name: user_name.to_string(),
            email: format!("{}@test.com", user_name.to_lowercase()),
            tags: vec![],
        }).await.unwrap();

        let result = add_uc.execute(AddUserToGroupCommand {
            user_hrn: user_view.hrn,
            group_hrn: group_view.hrn.clone(),
        }).await;

        assert!(result.is_ok());
    }

    // Verify all users are in the group
    let all_users = user_repo.find_all().await.unwrap();
    for user in all_users {
        assert_eq!(user.groups().len(), 1);
        assert_eq!(user.groups()[0].to_string(), group_view.hrn);
    }
}

#[tokio::test]
async fn test_add_user_to_multiple_groups() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    // Create a user
    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let user_view = create_user_uc.execute(CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@test.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Create multiple groups
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());

    let groups = vec!["developers", "designers", "managers"];

    for group_name in groups {
        let group_view = create_group_uc.execute(CreateGroupCommand {
            group_name: group_name.to_string(),
            tags: vec![],
        }).await.unwrap();

        let result = add_uc.execute(AddUserToGroupCommand {
            user_hrn: user_view.hrn.clone(),
            group_hrn: group_view.hrn,
        }).await;

        assert!(result.is_ok());
    }

    // Verify user is in all groups
    let users = user_repo.find_all().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].groups().len(), 3);
}

#[tokio::test]
async fn test_complex_user_group_relationships() {
    let user_repo = Arc::new(InMemoryUserRepository::new());
    let group_repo = Arc::new(InMemoryGroupRepository::new());

    let create_user_uc = create_user_feature::di::make_use_case(user_repo.clone());
    let create_group_uc = create_group_feature::di::make_use_case(group_repo.clone());
    let add_uc = add_user_to_group::di::make_use_case(user_repo.clone(), group_repo.clone());

    // Create groups
    let dev_group = create_group_uc.execute(CreateGroupCommand {
        group_name: "developers".to_string(),
        tags: vec![],
    }).await.unwrap();

    let ops_group = create_group_uc.execute(CreateGroupCommand {
        group_name: "operations".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Create users
    let alice = create_user_uc.execute(CreateUserCommand {
        name: "Alice".to_string(),
        email: "alice@test.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    let bob = create_user_uc.execute(CreateUserCommand {
        name: "Bob".to_string(),
        email: "bob@test.com".to_string(),
        tags: vec![],
    }).await.unwrap();

    // Alice is in both groups
    add_uc.execute(AddUserToGroupCommand {
        user_hrn: alice.hrn.clone(),
        group_hrn: dev_group.hrn.clone(),
    }).await.unwrap();

    add_uc.execute(AddUserToGroupCommand {
        user_hrn: alice.hrn,
        group_hrn: ops_group.hrn.clone(),
    }).await.unwrap();

    // Bob is only in developers
    add_uc.execute(AddUserToGroupCommand {
        user_hrn: bob.hrn,
        group_hrn: dev_group.hrn,
    }).await.unwrap();

    // Verify relationships
    let all_users = user_repo.find_all().await.unwrap();
    let alice_user = all_users.iter().find(|u| u.name == "Alice").unwrap();
    let bob_user = all_users.iter().find(|u| u.name == "Bob").unwrap();

    assert_eq!(alice_user.groups().len(), 2);
    assert_eq!(bob_user.groups().len(), 1);
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/di.rs">
use crate::features::attach_scp::adapter::{
    AccountRepositoryAdapter, OuRepositoryAdapter, ScpRepositoryAdapter,
};
use crate::features::attach_scp::use_case::AttachScpUseCase;
use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::application::ports::ou_repository::OuRepository;
use crate::shared::application::ports::scp_repository::ScpRepository;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Create an instance of the AttachScpUseCase with the provided repositories
pub fn attach_scp_use_case<
    SR: ScpRepository + std::marker::Sync + std::marker::Send,
    AR: AccountRepository + std::marker::Sync + std::marker::Send,
    OR: OuRepository + std::marker::Sync + std::marker::Send,
>(
    scp_repository: SR,
    account_repository: AR,
    ou_repository: OR,
) -> AttachScpUseCase<ScpRepositoryAdapter<SR>, AccountRepositoryAdapter<AR>, OuRepositoryAdapter<OR>>
{
    let scp_adapter = ScpRepositoryAdapter::new(scp_repository);
    let account_adapter = AccountRepositoryAdapter::new(account_repository);
    let ou_adapter = OuRepositoryAdapter::new(ou_repository);
    AttachScpUseCase::new(scp_adapter, account_adapter, ou_adapter)
}

/// Create an instance of the AttachScpUseCase with event bus integration
pub fn attach_scp_use_case_with_events<
    SR: ScpRepository + std::marker::Sync + std::marker::Send,
    AR: AccountRepository + std::marker::Sync + std::marker::Send,
    OR: OuRepository + std::marker::Sync + std::marker::Send,
>(
    scp_repository: SR,
    account_repository: AR,
    ou_repository: OR,
    event_bus: Arc<InMemoryEventBus>,
) -> AttachScpUseCase<ScpRepositoryAdapter<SR>, AccountRepositoryAdapter<AR>, OuRepositoryAdapter<OR>>
{
    let scp_adapter = ScpRepositoryAdapter::new(scp_repository);
    let account_adapter = AccountRepositoryAdapter::new(account_repository);
    let ou_adapter = OuRepositoryAdapter::new(ou_repository);
    AttachScpUseCase::new(scp_adapter, account_adapter, ou_adapter).with_event_publisher(event_bus)
}
</file>

<file path="crates/hodei-organizations/src/features/attach_scp/use_case.rs">
use crate::features::attach_scp::dto::{AttachScpCommand, AttachScpView};
use crate::features::attach_scp::error::AttachScpError;
use crate::features::attach_scp::ports::{
    AccountRepositoryPort, OuRepositoryPort, ScpRepositoryPort,
};
use crate::shared::domain::events::{ScpAttached, ScpTargetType};
use policies::domain::Hrn;
use shared::EventPublisher;
use shared::application::ports::event_bus::EventEnvelope;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Use case for attaching an SCP to an entity (Account or OU)
pub struct AttachScpUseCase<
    SRP: ScpRepositoryPort,
    ARP: AccountRepositoryPort,
    ORP: OuRepositoryPort,
> {
    scp_repository: SRP,
    account_repository: ARP,
    ou_repository: ORP,
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl<SRP: ScpRepositoryPort, ARP: AccountRepositoryPort, ORP: OuRepositoryPort>
    AttachScpUseCase<SRP, ARP, ORP>
{
    /// Create a new instance of the use case
    pub fn new(scp_repository: SRP, account_repository: ARP, ou_repository: ORP) -> Self {
        Self {
            scp_repository,
            account_repository,
            ou_repository,
            event_publisher: None,
        }
    }

    pub fn with_event_publisher(mut self, publisher: Arc<InMemoryEventBus>) -> Self {
        self.event_publisher = Some(publisher);
        self
    }

    /// Execute the use case
    pub async fn execute(
        &self,
        command: AttachScpCommand,
    ) -> Result<AttachScpView, AttachScpError> {
        // Parse HRNs
        let scp_hrn = Hrn::from_string(&command.scp_hrn)
            .ok_or_else(|| AttachScpError::ScpNotFound(command.scp_hrn.clone()))?;
        let target_hrn = Hrn::from_string(&command.target_hrn)
            .ok_or_else(|| AttachScpError::TargetNotFound(command.target_hrn.clone()))?;

        // Find the SCP
        let _scp = self
            .scp_repository
            .find_scp_by_hrn(&scp_hrn)
            .await?
            .ok_or_else(|| AttachScpError::ScpNotFound(command.scp_hrn.clone()))?;

        // Attach SCP based on target entity type
        let target_type = match target_hrn.resource_type.as_str() {
            "account" => {
                let mut account = self
                    .account_repository
                    .find_account_by_hrn(&target_hrn)
                    .await?
                    .ok_or_else(|| AttachScpError::TargetNotFound(command.target_hrn.clone()))?;
                account.attach_scp(scp_hrn.clone());
                self.account_repository.save_account(account).await?;
                ScpTargetType::Account
            }
            "ou" => {
                let mut ou = self
                    .ou_repository
                    .find_ou_by_hrn(&target_hrn)
                    .await?
                    .ok_or_else(|| AttachScpError::TargetNotFound(command.target_hrn.clone()))?;
                ou.attach_scp(scp_hrn.clone());
                self.ou_repository.save_ou(ou).await?;
                ScpTargetType::OrganizationalUnit
            }
            _ => {
                return Err(AttachScpError::InvalidTargetType(
                    target_hrn.resource_type.clone(),
                ));
            }
        };

        // Publish domain event
        if let Some(publisher) = &self.event_publisher {
            let event = ScpAttached {
                scp_hrn: scp_hrn.clone(),
                target_hrn: target_hrn.clone(),
                target_type,
                attached_at: chrono::Utc::now(),
            };

            let envelope = EventEnvelope::new(event)
                .with_metadata("aggregate_type".to_string(), "Scp".to_string());

            if let Err(e) = publisher.publish_with_envelope(envelope).await {
                tracing::warn!("Failed to publish ScpAttached event: {}", e);
                // Don't fail the use case if event publishing fails
            }
        }

        // Return the attach SCP view
        Ok(AttachScpView {
            scp_hrn: scp_hrn.to_string(),
            target_hrn: target_hrn.to_string(),
        })
    }
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/di.rs">
use crate::features::create_account::adapter::AccountRepositoryAdapter;
use crate::features::create_account::use_case::CreateAccountUseCase;
use crate::shared::application::ports::account_repository::AccountRepository;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

/// Create an instance of the CreateAccountUseCase with the provided repository
#[allow(dead_code)]
pub(crate) fn create_account_use_case<AR: AccountRepository + Send + Sync>(
    account_repository: AR,
    partition: String,
    account_id: String,
) -> CreateAccountUseCase<AccountRepositoryAdapter<AR>> {
    let adapter = AccountRepositoryAdapter::new(account_repository);
    CreateAccountUseCase::new(Arc::new(adapter), partition, account_id)
}

/// Create an instance of the CreateAccountUseCase with event bus integration
#[allow(dead_code)]
pub(crate) fn create_account_use_case_with_events<AR: AccountRepository + Send + Sync>(
    account_repository: AR,
    partition: String,
    account_id: String,
    event_bus: Arc<InMemoryEventBus>,
) -> CreateAccountUseCase<AccountRepositoryAdapter<AR>> {
    let adapter = AccountRepositoryAdapter::new(account_repository);
    CreateAccountUseCase::new(Arc::new(adapter), partition, account_id)
        .with_event_publisher(event_bus)
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/dto.rs">
use policies::domain::Hrn;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccountCommand {
    pub name: String,
    pub parent_hrn: Option<Hrn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountView {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Option<Hrn>,
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/mod.rs">
pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
#[cfg(test)]
pub mod mocks;
pub mod ports;
pub mod use_case;
#[cfg(test)]
pub mod use_case_test;
</file>

<file path="crates/hodei-organizations/src/features/create_account/use_case_test.rs">
use crate::features::create_account::di::create_account_use_case;
use crate::features::create_account::dto::CreateAccountCommand;
use crate::features::create_account::error::CreateAccountError;
use crate::shared::application::ports::account_repository::{
    AccountRepository, AccountRepositoryError,
};
use crate::shared::domain::account::Account;

use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// In-memory implementation of AccountRepository used across tests to
/// ensure the AccountRepositoryAdapter (constructed via the DI function)
/// is always exercised (eliminating dead_code warnings for struct/new()).
struct InMemoryAccountRepository {
    accounts: Arc<Mutex<HashMap<String, Account>>>,
}

impl InMemoryAccountRepository {
    fn new() -> Self {
        Self {
            accounts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl AccountRepository for InMemoryAccountRepository {
    async fn save(&self, account: &Account) -> Result<(), AccountRepositoryError> {
        let mut g = self.accounts.lock().unwrap();
        g.insert(account.hrn.to_string(), account.clone());
        Ok(())
    }

    async fn find_by_hrn(&self, hrn: &Hrn) -> Result<Option<Account>, AccountRepositoryError> {
        let g = self.accounts.lock().unwrap();
        Ok(g.get(&hrn.to_string()).cloned())
    }
}

#[tokio::test]
async fn test_create_account_success() {
    // Arrange
    let repo = InMemoryAccountRepository::new();
    let use_case = create_account_use_case(repo, "aws".to_string(), "123456789012".to_string());
    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "ou".to_string(),
        "ou-123".to_string(),
    );
    let command = CreateAccountCommand {
        name: "TestAccount".to_string(),
        parent_hrn: Some(parent_hrn.clone()),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok());
    let account_view = result.unwrap();
    assert_eq!(account_view.name, "TestAccount");
    assert_eq!(account_view.parent_hrn, Some(parent_hrn));
    assert!(!account_view.hrn.to_string().is_empty());
}

#[tokio::test]
async fn test_create_account_empty_name() {
    // Arrange
    let repo = InMemoryAccountRepository::new();
    let use_case = create_account_use_case(repo, "aws".to_string(), "123456789012".to_string());
    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "ou".to_string(),
        "ou-123".to_string(),
    );
    let command = CreateAccountCommand {
        name: "".to_string(),
        parent_hrn: Some(parent_hrn),
    };

    // Act
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(error, CreateAccountError::InvalidAccountName));
}

#[tokio::test]
async fn test_create_account_with_di() {
    let repo = InMemoryAccountRepository::new();
    let use_case = create_account_use_case(repo, "aws".to_string(), "123456789012".to_string());

    let parent_hrn = Hrn::new(
        "aws".to_string(),
        "organizations".to_string(),
        "123456789012".to_string(),
        "ou".to_string(),
        "ou-di".to_string(),
    );

    let command = CreateAccountCommand {
        name: "DIAccount".to_string(),
        parent_hrn: Some(parent_hrn.clone()),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());
    let view = result.unwrap();
    assert_eq!(view.name, "DIAccount");
    assert_eq!(view.parent_hrn, Some(parent_hrn));
}
</file>

<file path="crates/hodei-organizations/src/features/create_account/use_case.rs">
use crate::features::create_account::dto::{AccountView, CreateAccountCommand};
use crate::features::create_account::error::CreateAccountError;
use crate::features::create_account::ports::AccountPersister;
use crate::shared::domain::account::Account;
use crate::shared::domain::events::AccountCreated;
use policies::domain::Hrn;
use shared::EventPublisher;
use shared::application::ports::event_bus::EventEnvelope;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

pub struct CreateAccountUseCase<AP: AccountPersister> {
    persister: Arc<AP>,
    /// Partition for HRN generation (e.g., "aws", "hodei")
    partition: String,
    /// Account identifier for HRN generation (e.g., "default", account_id)
    account_id: String,
    /// Optional event publisher for domain events
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl<AP: AccountPersister> CreateAccountUseCase<AP> {
    pub fn new(persister: Arc<AP>, partition: String, account_id: String) -> Self {
        Self {
            persister,
            partition,
            account_id,
            event_publisher: None,
        }
    }

    pub fn with_event_publisher(mut self, publisher: Arc<InMemoryEventBus>) -> Self {
        self.event_publisher = Some(publisher);
        self
    }

    pub async fn execute(
        &self,
        command: CreateAccountCommand,
    ) -> Result<AccountView, CreateAccountError> {
        // Validar el nombre de la cuenta
        if command.name.is_empty() {
            return Err(CreateAccountError::InvalidAccountName);
        }

        // Generar HRN para la nueva cuenta (centralized generation)
        // Format: hrn:partition:organizations:account_id:account/account_name
        let hrn = Hrn::new(
            self.partition.clone(),
            "organizations".to_string(),
            self.account_id.clone(),
            "account".to_string(),
            command.name.clone(),
        );

        // Crear la cuenta
        let account = Account::new(hrn.clone(), command.name.clone(), command.parent_hrn);

        // Guardar la cuenta
        self.persister.save(account.clone()).await?;

        // Publish domain event
        if let Some(publisher) = &self.event_publisher {
            let event = AccountCreated {
                account_hrn: account.hrn.clone(),
                name: account.name.clone(),
                parent_hrn: account.parent_hrn.clone(),
                created_at: chrono::Utc::now(),
            };

            let envelope = EventEnvelope::new(event)
                .with_metadata("aggregate_type".to_string(), "Account".to_string());

            if let Err(e) = publisher.publish_with_envelope(envelope).await {
                tracing::warn!("Failed to publish AccountCreated event: {}", e);
                // Don't fail the use case if event publishing fails
            }
        }

        // Devolver la vista de la cuenta
        Ok(AccountView {
            hrn: account.hrn,
            name: account.name,
            parent_hrn: account.parent_hrn,
        })
    }
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/dto.rs">
use cedar_policy::PolicySet;
use serde::{Deserialize, Serialize};

/// Query to get effective SCPs for a resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEffectiveScpsQuery {
    /// HRN of the target entity (Account or OU)
    pub resource_hrn: String,
}

/// Response containing effective SCPs as a Cedar PolicySet
/// This is the PUBLIC interface - does not expose internal entities
#[derive(Debug, Clone)]
pub struct EffectiveScpsResponse {
    /// Cedar PolicySet containing all effective SCPs
    /// This can be directly used by the authorization engine
    pub policies: PolicySet,
    /// HRN of the target entity (for logging/debugging)
    pub target_hrn: String,
}

impl EffectiveScpsResponse {
    pub fn new(policies: PolicySet, target_hrn: String) -> Self {
        Self {
            policies,
            target_hrn,
        }
    }
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/mod.rs">
pub mod adapter;
pub mod di;
pub mod dto;
pub mod error;
pub mod mocks;
pub mod ports;
pub mod use_case;

// Re-exports públicos para acceso externo
pub use dto::{EffectiveScpsResponse, GetEffectiveScpsQuery};
pub use error::GetEffectiveScpsError;
pub use use_case::GetEffectiveScpsUseCase;
</file>

<file path="crates/hodei-organizations/src/features/move_account/use_case.rs">
use crate::features::move_account::dto::MoveAccountCommand;
use crate::features::move_account::error::MoveAccountError;
use crate::features::move_account::ports::{MoveAccountUnitOfWork, MoveAccountUnitOfWorkFactory};
use std::sync::Arc;

/// Transactional MoveAccountUseCase using UnitOfWork pattern
///
/// This implementation ensures atomic operations across multiple repositories
/// by using the UnitOfWork pattern for transaction management.
pub struct MoveAccountUseCase<UWF: MoveAccountUnitOfWorkFactory> {
    uow_factory: Arc<UWF>,
}

impl<UWF: MoveAccountUnitOfWorkFactory> MoveAccountUseCase<UWF> {
    pub fn new(uow_factory: Arc<UWF>) -> Self {
        Self { uow_factory }
    }

    pub async fn execute(&self, command: MoveAccountCommand) -> Result<(), MoveAccountError> {
        // Create a new UnitOfWork for this operation
        let mut uow = self.uow_factory.create().await?;

        // Begin the transaction
        uow.begin().await?;

        // Execute the business logic within the transaction
        let result = self.execute_within_transaction(&command, &mut uow).await;

        // Commit or rollback based on the result
        match result {
            Ok(_) => {
                uow.commit().await?;
                Ok(())
            }
            Err(e) => {
                // Attempt to rollback, but don't hide the original error
                if let Err(rollback_err) = uow.rollback().await {
                    eprintln!("Failed to rollback transaction: {}", rollback_err);
                }
                Err(e)
            }
        }
    }

    async fn execute_within_transaction<UOW: MoveAccountUnitOfWork>(
        &self,
        command: &MoveAccountCommand,
        uow: &mut UOW,
    ) -> Result<(), MoveAccountError> {
        // Get repositories from the UnitOfWork
        let account_repo = uow.accounts();
        let ou_repo = uow.ous();

        // 1. Cargar la Account a mover
        let mut account = account_repo
            .find_by_hrn(&command.account_hrn)
            .await?
            .ok_or(MoveAccountError::AccountNotFound)?;

        // 2. Cargar la OU de origen
        let mut source_ou = ou_repo
            .find_by_hrn(&command.source_ou_hrn)
            .await?
            .ok_or(MoveAccountError::SourceOuNotFound)?;

        // 3. Cargar la OU de destino
        let mut target_ou = ou_repo
            .find_by_hrn(&command.target_ou_hrn)
            .await?
            .ok_or(MoveAccountError::TargetOuNotFound)?;

        // 4. Llamar a source_ou.remove_child_account(...)
        source_ou.remove_child_account(&account.hrn);

        // 5. Llamar a account.set_parent(...)
        account.parent_hrn = Some(command.target_ou_hrn.clone());

        // 6. Llamar a target_ou.add_child_account(...)
        target_ou.add_child_account(account.hrn.clone());

        // 7. Guardar los tres agregados modificados (account, source_ou, target_ou)
        // Todas las operaciones ocurren dentro de la misma transacción
        account_repo.save(&account).await?;
        ou_repo.save(&source_ou).await?;
        ou_repo.save(&target_ou).await?;

        Ok(())
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/application/ports/mod.rs">
pub mod account_repository;
pub mod ou_repository;
pub mod scp_repository;

// Re-export error types for convenience
pub use account_repository::AccountRepositoryError;
pub use ou_repository::OuRepositoryError;
pub use scp_repository::ScpRepositoryError;
</file>

<file path="crates/hodei-organizations/src/shared/domain/account.rs">
use policies::shared::Hrn;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Option<Hrn>,
    pub attached_scps: HashSet<Hrn>,
}

impl Account {
    pub fn new(hrn: Hrn, name: String, parent_hrn: Option<Hrn>) -> Self {
        Self {
            hrn,
            name,
            parent_hrn,
            attached_scps: HashSet::new(),
        }
    }

    pub fn set_parent(&mut self, parent_hrn: Hrn) {
        self.parent_hrn = Some(parent_hrn);
    }

    pub fn attach_scp(&mut self, scp_hrn: Hrn) {
        self.attached_scps.insert(scp_hrn);
    }

    pub fn detach_scp(&mut self, scp_hrn: &Hrn) -> bool {
        self.attached_scps.remove(scp_hrn)
    }

    pub fn has_scp(&self, scp_hrn: &Hrn) -> bool {
        self.attached_scps.contains(scp_hrn)
    }
}
</file>

<file path="crates/hodei-organizations/src/lib.rs">
//! hodei-organizations: Organization structure and Service Control Policies
//!
//! This crate manages organizational units, accounts, and SCPs.
//! External access is ONLY through features (use cases).
//!
//! # Architecture
//! - Internal: domain entities, repositories, adapters
//! - External: ONLY features/use cases with Commands/Queries/DTOs

pub mod features;
pub mod shared;

// ❌ NO exportar entidades de dominio - son INTERNAS
// Solo se accede a este crate a través de sus casos de uso (features)

// ✅ Re-export features/casos de uso para acceso externo
pub use features::{
    attach_scp::use_case::AttachScpUseCase,
    create_account::use_case::CreateAccountUseCase,
    create_ou::use_case::CreateOuUseCase,
    get_effective_scps::{
        dto::{EffectiveScpsResponse, GetEffectiveScpsQuery},
        use_case::GetEffectiveScpsUseCase,
    },
};
</file>

<file path="crates/policies/src/shared/application/parallel.rs">
use cedar_policy::{Authorizer, Context, Entities, Entity, EntityUid, Policy, PolicySet, Request, RestrictedExpression};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;
use tokio::time::{timeout, Duration};

/// Scenario description compatible with Cedar
#[derive(Clone, Debug)]
pub struct AuthScenario {
    pub name: String,
    pub principal: String,
    pub action: String,
    pub resource: String,
    pub context: Option<HashMap<String, serde_json::Value>>,
}

/// mpsc-based pipeline with back-pressure and explicit worker count
pub async fn evaluate_scenarios_channel(
    policies: &PolicySet,
    entities: &Entities,
    scenarios: Vec<AuthScenario>,
    timeout_ms: Option<u64>,
    workers: usize,
    buffer: usize,
) -> Result<(Vec<AuthOutcome>, ParallelStats), String> {
    let (tx_in, rx_in) = mpsc::channel::<AuthScenario>(buffer);
    let (tx_out, mut rx_out) = mpsc::channel::<AuthOutcome>(buffer);

    // clone shared inputs
    let policies = policies.clone();
    let entities = entities.clone();

    // Producer
    let scenarios_total = scenarios.len();
    tokio::spawn(async move {
        for sc in scenarios.into_iter() {
            if tx_in.send(sc).await.is_err() { break; }
        }
        // drop sender to close channel
    });

    // Workers
    let rx_arc = Arc::new(Mutex::new(rx_in));
    for _ in 0..workers {
        let rx = rx_arc.clone();
        let tx = tx_out.clone();
        let policies = policies.clone();
        let entities = entities.clone();
        tokio::spawn(async move {
            let authorizer = Authorizer::new();
            loop {
                let sc_opt = { rx.lock().await.recv().await };
                let Some(sc) = sc_opt else { break };
                let principal = match EntityUid::from_str(&sc.principal) { Ok(v) => v, Err(_) => continue };
                let action = match EntityUid::from_str(&sc.action) { Ok(v) => v, Err(_) => continue };
                let resource = match EntityUid::from_str(&sc.resource) { Ok(v) => v, Err(_) => continue };
                let context = {
                    let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
                    if let Some(ctx) = sc.context.as_ref() {
                        for (k, v) in ctx { if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); } }
                    }
                    Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
                };
                let request = match Request::new(principal, action, resource, context, None) { Ok(v) => v, Err(_) => continue };
                let nm = sc.name.clone();
                let nm_to = nm.clone();
                let a = &authorizer;
                let p = &policies;
                let e = &entities;
                let fut = async move {
                    let start = std::time::Instant::now();
                    let resp = a.is_authorized(&request, p, e);
                    let allow = resp.decision() == cedar_policy::Decision::Allow;
                    let reasons: Vec<String> = resp.diagnostics().reason().map(|r| r.to_string()).collect();
                    let eval_time_us = start.elapsed().as_micros() as u64;
                    AuthOutcome { name: nm.clone(), allow, eval_time_us, reasons }
                };
                let outcome = if let Some(ms) = timeout_ms {
                    match timeout(Duration::from_millis(ms), fut).await {
                        Ok(o) => o,
                        Err(_elapsed) => AuthOutcome { name: nm_to, allow: false, eval_time_us: 0, reasons: vec!["timeout".to_string()] },
                    }
                } else { fut.await };
                let _ = tx.send(outcome).await;
            }
        });
    }
    // rx_in is moved into rx_arc
    drop(tx_out);

    // Collector
    let mut outcomes = Vec::new();
    let mut stats = ParallelStats { scenarios_total, ..Default::default() };
    while let Some(out) = rx_out.recv().await {
        if out.eval_time_us == 0 && out.reasons.iter().any(|r| r == "timeout") { stats.timeouts += 1; }
        stats.total_eval_time_us += out.eval_time_us;
        outcomes.push(out);
    }
    Ok((outcomes, stats))
}

/// Evaluate scenarios in parallel and return the first outcome matching the predicate.
/// Early-cancels remaining work using a shared AtomicBool.
pub async fn evaluate_until_first<F>(
    policies: &PolicySet,
    entities: &Entities,
    scenarios: Vec<AuthScenario>,
    timeout_ms: Option<u64>,
    workers: usize,
    buffer: usize,
    predicate: F,
) -> Result<Option<AuthOutcome>, String>
where
    F: Fn(&AuthOutcome) -> bool + Send + Sync + 'static,
{
    let predicate = Arc::new(predicate);
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx_in, rx_in) = mpsc::channel::<AuthScenario>(buffer);
    let (tx_out, mut rx_out) = mpsc::channel::<AuthOutcome>(buffer);

    // clones
    let policies = policies.clone();
    let entities = entities.clone();

    // producer
    tokio::spawn({
        let cancel = cancel.clone();
        async move {
            for sc in scenarios.into_iter() {
                if cancel.load(Ordering::Relaxed) { break; }
                if tx_in.send(sc).await.is_err() { break; }
            }
        }
    });

    // workers
    let rx_arc = Arc::new(Mutex::new(rx_in));
    for _ in 0..workers {
        let rx = rx_arc.clone();
        let tx = tx_out.clone();
        let policies = policies.clone();
        let entities = entities.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let authorizer = Authorizer::new();
            while !cancel.load(Ordering::Relaxed) {
                let sc_opt = { rx.lock().await.recv().await };
                let Some(sc) = sc_opt else { break };
                let principal = match EntityUid::from_str(&sc.principal) { Ok(v) => v, Err(_) => continue };
                let action = match EntityUid::from_str(&sc.action) { Ok(v) => v, Err(_) => continue };
                let resource = match EntityUid::from_str(&sc.resource) { Ok(v) => v, Err(_) => continue };
                let context = {
                    let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
                    if let Some(ctx) = sc.context.as_ref() { for (k, v) in ctx { if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); } } }
                    Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
                };
                let request = match Request::new(principal, action, resource, context, None) { Ok(v) => v, Err(_) => continue };
                let nm = sc.name.clone();
                let nm_to = nm.clone();
                let a = &authorizer;
                let p = &policies;
                let e = &entities;
                let fut = async move {
                    let start = std::time::Instant::now();
                    let resp = a.is_authorized(&request, p, e);
                    let allow = resp.decision() == cedar_policy::Decision::Allow;
                    let reasons: Vec<String> = resp.diagnostics().reason().map(|r| r.to_string()).collect();
                    let eval_time_us = start.elapsed().as_micros() as u64;
                    AuthOutcome { name: nm, allow, eval_time_us, reasons }
                };
                let outcome = if let Some(ms) = timeout_ms {
                    match timeout(Duration::from_millis(ms), fut).await { Ok(o) => o, Err(_elapsed) => AuthOutcome { name: nm_to, allow: false, eval_time_us: 0, reasons: vec!["timeout".to_string()] } }
                } else { fut.await };
                if cancel.load(Ordering::Relaxed) { break; }
                let _ = tx.send(outcome).await;
            }
        });
    }
    // rx_in moved into rx_arc
    drop(tx_out);

    // collector
    while let Some(out) = rx_out.recv().await {
        if predicate.as_ref()(&out) {
            cancel.store(true, Ordering::Relaxed);
            return Ok(Some(out));
        }
    }
    Ok(None)
}

#[derive(Clone, Debug)]
pub struct AuthOutcome {
    pub name: String,
    pub allow: bool,
    pub eval_time_us: u64,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ParallelStats {
    pub scenarios_total: usize,
    pub timeouts: usize,
    pub total_eval_time_us: u64,
}

pub fn build_policy_set(policies: &[String]) -> Result<PolicySet, String> {
    let mut pset = PolicySet::new();
    for (i, pstr) in policies.iter().enumerate() {
        let pol: Policy = pstr
            .parse()
            .map_err(|e| format!("policy[{}] parse error: {}", i, e))?;
        pset.add(pol)
            .map_err(|e| format!("policy[{}] add error: {}", i, e))?;
    }
    Ok(pset)
}

pub fn build_entities(defs: &[(String, HashMap<String, serde_json::Value>, Vec<String>)]) -> Result<Entities, String> {
    if defs.is_empty() { return Ok(Entities::empty()); }
    let mut out = Vec::with_capacity(defs.len());
    for (uid_str, attrs_map, parents_vec) in defs {
        let uid = EntityUid::from_str(uid_str).map_err(|e| e.to_string())?;
        let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();
        for (k, v) in attrs_map.iter() {
            if let Some(expr) = json_to_expr(v) { attrs.insert(k.clone(), expr); }
        }
        let mut parents: HashSet<EntityUid> = HashSet::new();
        for p in parents_vec.iter() { parents.insert(EntityUid::from_str(p).map_err(|e| e.to_string())?); }
        let ent = Entity::new(uid, attrs, parents).map_err(|e| e.to_string())?;
        out.push(ent);
    }
    Entities::from_entities(out, None).map_err(|e| e.to_string())
}

pub fn json_to_expr(v: &serde_json::Value) -> Option<RestrictedExpression> {
    match v {
        serde_json::Value::String(s) => Some(RestrictedExpression::new_string(s.clone())),
        serde_json::Value::Bool(b) => Some(RestrictedExpression::new_bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(RestrictedExpression::new_long(i))
            } else {
                n.as_f64().map(|f| RestrictedExpression::new_decimal(f.to_string()))
            }
        }
        serde_json::Value::Array(arr) => {
            let elems: Vec<RestrictedExpression> = arr.iter().filter_map(json_to_expr).collect();
            Some(RestrictedExpression::new_set(elems))
        }
        serde_json::Value::Object(map) => {
            let mut rec: BTreeMap<String, RestrictedExpression> = BTreeMap::new();
            for (k, val) in map.iter() { if let Some(expr) = json_to_expr(val) { rec.insert(k.clone(), expr); } }
            RestrictedExpression::new_record(rec).ok()
        }
        serde_json::Value::Null => None,
    }
}

pub async fn evaluate_scenarios_joinset(
    policies: &PolicySet,
    entities: &Entities,
    scenarios: Vec<AuthScenario>,
    timeout_ms: Option<u64>,
    max_concurrency: usize,
) -> Result<Vec<AuthOutcome>, String> {
    let mut set: JoinSet<Result<AuthOutcome, String>> = JoinSet::new();
    let mut iter = scenarios.into_iter();

    // seed
    for _ in 0..max_concurrency {
        if let Some(sc) = iter.next() { spawn_eval(&mut set, policies.clone(), entities.clone(), sc, timeout_ms); }
    }

    let mut outcomes = Vec::new();
    while let Some(joined) = set.join_next().await {
        let out = match joined { Ok(r) => r, Err(e) => Err(e.to_string()) }?;
        outcomes.push(out);
        if let Some(sc) = iter.next() { spawn_eval(&mut set, policies.clone(), entities.clone(), sc, timeout_ms); }
    }
    Ok(outcomes)
}

fn spawn_eval(
    set: &mut JoinSet<Result<AuthOutcome, String>>,
    policies: PolicySet,
    entities: Entities,
    sc: AuthScenario,
    timeout_ms: Option<u64>,
) {
    set.spawn(async move {
        let authorizer = Authorizer::new();
        let principal = EntityUid::from_str(&sc.principal).map_err(|e| e.to_string())?;
        let action = EntityUid::from_str(&sc.action).map_err(|e| e.to_string())?;
        let resource = EntityUid::from_str(&sc.resource).map_err(|e| e.to_string())?;
        let context = {
            let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
            if let Some(ctx) = sc.context.as_ref() {
                for (k, v) in ctx { if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); } }
            }
            Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
        };
        let request = Request::new(principal, action, resource, context, None).map_err(|e| e.to_string())?;
        let name = sc.name.clone();
        let name_to = name.clone();
        let fut = async move {
            let start = std::time::Instant::now();
            let resp = authorizer.is_authorized(&request, &policies, &entities);
            let allow = resp.decision() == cedar_policy::Decision::Allow;
            let reasons: Vec<String> = resp.diagnostics().reason().map(|r| r.to_string()).collect();
            let eval_time_us = start.elapsed().as_micros() as u64;
            Ok(AuthOutcome { name: name.clone(), allow, eval_time_us, reasons })
        };
        if let Some(ms) = timeout_ms {
            match timeout(Duration::from_millis(ms), fut).await {
                Ok(r) => r,
                Err(_elapsed) => Ok(AuthOutcome { name: name_to, allow: false, eval_time_us: 0, reasons: vec!["timeout".to_string()] }),
            }
        } else {
            fut.await
        }
    });
}
</file>

<file path="crates/policies/src/shared/infrastructure/mod.rs">
// Facade raíz del crate policies (estructura hexagonal interna)

// Infrastructure layer modules
pub mod surreal;
</file>

<file path="crates/policies/tests/delete_policy_integration_test.rs">
use cedar_policy::Policy;
use policies::features::delete_policy::di::make_delete_policy_use_case_mem;
use policies::features::delete_policy::dto::DeletePolicyCommand;
use policies::features::delete_policy::use_case::DeletePolicyError;

#[tokio::test]
async fn test_delete_policy_integration_success() {
    // Arrange: Create use case and add a policy
    let (delete_uc, engine) = make_delete_policy_use_case_mem()
        .await
        .expect("Failed to create delete_policy use case");

    // Add a policy
    let policy_src = r#"permit(principal, action, resource);"#;
    let policy: Policy = policy_src.parse().expect("parse policy");
    let policy_id = policy.id().to_string();
    engine
        .store
        .add_policy(policy.clone())
        .await
        .expect("add policy");

    // Verify it exists
    let retrieved = engine
        .store
        .get_policy(&policy_id)
        .await
        .expect("get policy");
    assert!(retrieved.is_some());

    // Act: Delete the policy
    let cmd = DeletePolicyCommand::new(policy_id.clone());
    let result = delete_uc.execute(&cmd).await;

    // Assert: Should succeed
    assert!(result.is_ok());
    assert!(result.unwrap());

    // Verify it's gone
    let retrieved_after = engine
        .store
        .get_policy(&policy_id)
        .await
        .expect("get policy after delete");
    assert!(retrieved_after.is_none());
}

#[tokio::test]
async fn test_delete_policy_integration_not_found() {
    // Arrange: Create use case with empty storage
    let (delete_uc, _engine) = make_delete_policy_use_case_mem()
        .await
        .expect("Failed to create delete_policy use case");

    // Act: Try to delete non-existent policy
    let cmd = DeletePolicyCommand::new("nonexistent_policy_id");
    let result = delete_uc.execute(&cmd).await;

    // Assert: Should return NotFound error
    assert!(result.is_err());
    match result {
        Err(DeletePolicyError::NotFound(id)) => {
            assert_eq!(id, "nonexistent_policy_id");
        }
        _ => panic!("Expected NotFound error"),
    }
}

#[tokio::test]
async fn test_delete_policy_integration_invalid_id() {
    // Arrange: Create use case
    let (delete_uc, _engine) = make_delete_policy_use_case_mem()
        .await
        .expect("Failed to create delete_policy use case");

    // Act: Try to delete with empty ID
    let cmd = DeletePolicyCommand::new("");
    let result = delete_uc.execute(&cmd).await;

    // Assert: Should return InvalidCommand error
    assert!(result.is_err());
    match result {
        Err(DeletePolicyError::InvalidCommand(_)) => {}
        _ => panic!("Expected InvalidCommand error"),
    }
}

#[tokio::test]
async fn test_delete_policy_integration_idempotent() {
    // Arrange: Create use case and add a policy
    let (delete_uc, engine) = make_delete_policy_use_case_mem()
        .await
        .expect("Failed to create delete_policy use case");

    // Add a policy
    let policy_src = r#"permit(principal, action, resource);"#;
    let policy: Policy = policy_src.parse().expect("parse policy");
    let policy_id = policy.id().to_string();
    engine
        .store
        .add_policy(policy.clone())
        .await
        .expect("add policy");

    // Verify it exists
    let retrieved = engine
        .store
        .get_policy(&policy_id)
        .await
        .expect("get policy");
    assert!(retrieved.is_some(), "Policy should exist before deletion");

    // Act: Delete the policy
    let cmd = DeletePolicyCommand::new(policy_id.clone());
    let result = delete_uc.execute(&cmd).await;
    assert!(result.is_ok(), "First deletion should succeed");

    // Assert: Policy is gone
    let retrieved_after = engine
        .store
        .get_policy(&policy_id)
        .await
        .expect("get policy after delete");
    assert!(retrieved_after.is_none(), "Policy should be deleted");

    // Act: Try to delete again
    let cmd2 = DeletePolicyCommand::new(policy_id.clone());
    let result2 = delete_uc.execute(&cmd2).await;

    // Assert: Should return NotFound error
    assert!(
        result2.is_err(),
        "Second deletion should fail with NotFound"
    );
    match result2 {
        Err(DeletePolicyError::NotFound(_)) => {}
        _ => panic!("Expected NotFound error on second deletion"),
    }
}
</file>

<file path="crates/policies/tests/hodei_entity_test.rs">
//! Test to verify the HodeiEntity implementation with RestrictedExpression

use cedar_policy::{Entity, EntityUid, RestrictedExpression, Schema, SchemaFragment};
use std::collections::HashMap;
use std::str::FromStr;

/// Example implementation of HodeiEntity for testing
#[derive(Debug)]
struct TestUser {
    id: String,
    name: String,
    email: String,
    groups: Vec<String>,
    tags: Vec<String>,
}

impl TestUser {
    fn new(
        id: String,
        name: String,
        email: String,
        groups: Vec<String>,
        tags: Vec<String>,
    ) -> Self {
        Self {
            id,
            name,
            email,
            groups,
            tags,
        }
    }

    fn euid(&self) -> EntityUid {
        EntityUid::from_str(&format!("User::\"{}\"", self.id)).unwrap()
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );
        attrs.insert(
            "email".to_string(),
            RestrictedExpression::new_string(self.email.clone()),
        );

        // For collections, we use new_set
        let group_expressions: Vec<RestrictedExpression> = self
            .groups
            .iter()
            .map(|group| RestrictedExpression::new_string(group.clone()))
            .collect();
        attrs.insert(
            "groups".to_string(),
            RestrictedExpression::new_set(group_expressions),
        );

        let tag_expressions: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|tag| RestrictedExpression::new_string(tag.clone()))
            .collect();
        attrs.insert(
            "tags".to_string(),
            RestrictedExpression::new_set(tag_expressions),
        );

        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        // In a real implementation, this would convert group names to EntityUids
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hodei_entity_implementation() {
        let user = TestUser::new(
            "alice".to_string(),
            "Alice Smith".to_string(),
            "alice@example.com".to_string(),
            vec!["developers".to_string(), "admins".to_string()],
            vec!["employee".to_string(), "fulltime".to_string()],
        );

        let attributes = user.attributes();
        assert_eq!(attributes.len(), 4);
        assert!(attributes.contains_key("name"));
        assert!(attributes.contains_key("email"));
        assert!(attributes.contains_key("groups"));
        assert!(attributes.contains_key("tags"));

        let entity = Entity::new(
            user.euid(),
            attributes,
            user.parents().into_iter().collect(),
        );

        assert!(entity.is_ok());
    }

    #[test]
    fn test_cedar_integration() -> Result<(), Box<dyn std::error::Error>> {
        // Create a simple schema
        let schema_str = r#"
        entity User {
            name: String,
            email: String,
            groups: Set<String>,
            tags: Set<String>
        };
        
        action access appliesTo {
            principal: User,
            resource: User
        };
        "#;

        let (schema_fragment, _) = SchemaFragment::from_cedarschema_str(schema_str)?;
        let _schema = Schema::from_schema_fragments([schema_fragment])?;

        // Create a user entity
        let user = TestUser::new(
            "alice".to_string(),
            "Alice Smith".to_string(),
            "alice@example.com".to_string(),
            vec!["developers".to_string(), "admins".to_string()],
            vec!["employee".to_string(), "fulltime".to_string()],
        );

        let entity = Entity::new(
            user.euid(),
            user.attributes(),
            user.parents().into_iter().collect(),
        )?;

        // Validate that the entity conforms to the schema
        assert_eq!(entity.uid().to_string(), r#"User::"alice""#);

        Ok(())
    }
}
</file>

<file path="crates/policies/tests/list_policies_integration_test.rs">
use cedar_policy::Policy;
use policies::features::list_policies::di::make_list_policies_use_case_mem;
use policies::features::list_policies::dto::ListPoliciesQuery;

#[tokio::test]
async fn test_list_policies_integration_empty() {
    // Arrange: Create use case with empty storage
    let (list_uc, _engine) = make_list_policies_use_case_mem()
        .await
        .expect("Failed to create list_policies use case");

    let query = ListPoliciesQuery::new();

    // Act: List policies
    let result = list_uc.execute(&query).await;

    // Assert: Should return empty list
    assert!(result.is_ok());
    let policies = result.unwrap();
    assert_eq!(policies.len(), 0);
}

#[tokio::test]
async fn test_list_policies_integration_with_data() {
    // Arrange: Create use case and add a policy
    let (list_uc, engine) = make_list_policies_use_case_mem()
        .await
        .expect("Failed to create list_policies use case");

    // Add a policy using the store
    let policy_src = r#"permit(principal, action, resource);"#;
    let policy: Policy = policy_src.parse().expect("parse policy");
    engine
        .store
        .add_policy(policy.clone())
        .await
        .expect("add policy");

    let query = ListPoliciesQuery::new();

    // Act: List policies
    let result = list_uc.execute(&query).await;

    // Assert: Should return at least 1 policy
    assert!(result.is_ok());
    let policies = result.unwrap();
    assert!(
        policies.len() >= 1,
        "Expected at least 1 policy, got {}",
        policies.len()
    );
}

#[tokio::test]
async fn test_list_policies_integration_after_create_and_delete() {
    // Arrange: Create use case and add a policy
    let (list_uc, engine) = make_list_policies_use_case_mem()
        .await
        .expect("Failed to create list_policies use case");

    // Add a policy
    let policy_src = r#"permit(principal, action, resource);"#;
    let policy: Policy = policy_src.parse().expect("parse policy");
    let policy_id = policy.id().to_string();
    engine
        .store
        .add_policy(policy.clone())
        .await
        .expect("add policy");

    // Verify it's listed
    let query = ListPoliciesQuery::new();
    let policies = list_uc.execute(&query).await.expect("list policies");
    assert_eq!(policies.len(), 1);

    // Delete the policy
    engine
        .store
        .remove_policy(&policy_id)
        .await
        .expect("remove policy");

    // Act: List policies again
    let policies_after = list_uc
        .execute(&query)
        .await
        .expect("list policies after delete");

    // Assert: Should be empty
    assert_eq!(policies_after.len(), 0);
}
</file>

<file path="crates/policies/tests/schema_rendering_final_test.rs">
use async_trait::async_trait;
use cedar_policy::{EntityTypeName, EntityUid, Policy, PolicySet, RestrictedExpression, Schema};
/// Tests para verificar el rendering final del schema generado por el EngineBuilder
///
/// Estos tests registran diferentes tipos de entidades y acciones para validar
/// que el schema final se genera correctamente con namespaces, atributos y relaciones.
/// Usan validación de Cedar como fuente principal de verdad.

use policies::shared::application::EngineBuilder;
use policies::shared::domain::ports::{
    Action, AttributeType, HodeiEntity, HodeiEntityType, PolicyStorage, Principal, Resource,
    StorageError,
};
use policies::shared::Hrn;
use regex::Regex;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

// ============================================================================
// Mock Storage
// ============================================================================

struct MockStorage;

#[async_trait]
impl PolicyStorage for MockStorage {
    async fn save_policy(&self, _policy: &Policy) -> Result<(), StorageError> {
        Ok(())
    }
    async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
        Ok(true)
    }
    async fn get_policy_by_id(&self, _id: &str) -> Result<Option<Policy>, StorageError> {
        Ok(None)
    }
    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError> {
        Ok(vec![])
    }
}

// ============================================================================
// Mock IAM Entities (Principals)
// ============================================================================

struct IamUser {
    hrn: Hrn,
}

impl HodeiEntityType for IamUser {
    fn service_name() -> &'static str {
        "iam"
    }
    fn resource_type_name() -> &'static str {
        "User"
    }
    fn is_principal_type() -> bool {
        true
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("email", AttributeType::Primitive("String")),
            ("name", AttributeType::Primitive("String")),
            ("active", AttributeType::Primitive("Bool")),
            ("roles", AttributeType::Set(Box::new(AttributeType::Primitive("String")))),
        ]
    }
}

impl HodeiEntity for IamUser {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Principal for IamUser {}

struct IamGroup {
    hrn: Hrn,
}

impl HodeiEntityType for IamGroup {
    fn service_name() -> &'static str {
        "iam"
    }
    fn resource_type_name() -> &'static str {
        "Group"
    }
    fn is_principal_type() -> bool {
        true
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("description", AttributeType::Primitive("String")),
        ]
    }
}

impl HodeiEntity for IamGroup {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Principal for IamGroup {}

// ============================================================================
// Mock Artifact Entities (Resources)
// ============================================================================

struct ArtifactPackage {
    hrn: Hrn,
}

impl HodeiEntityType for ArtifactPackage {
    fn service_name() -> &'static str {
        "artifact"
    }
    fn resource_type_name() -> &'static str {
        "Package"
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("version", AttributeType::Primitive("String")),
            ("type", AttributeType::Primitive("String")),
            ("size", AttributeType::Primitive("Long")),
            ("tags", AttributeType::Set(Box::new(AttributeType::Primitive("String")))),
        ]
    }
}

impl HodeiEntity for ArtifactPackage {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Resource for ArtifactPackage {}

struct ArtifactRepository {
    hrn: Hrn,
}

impl HodeiEntityType for ArtifactRepository {
    fn service_name() -> &'static str {
        "artifact"
    }
    fn resource_type_name() -> &'static str {
        "Repository"
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("visibility", AttributeType::Primitive("String")),
            ("ownerId", AttributeType::Primitive("String")),
        ]
    }
}

impl HodeiEntity for ArtifactRepository {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Resource for ArtifactRepository {}

// ============================================================================
// Mock Actions
// ============================================================================

struct ReadPackageAction;

impl Action for ReadPackageAction {
    fn name() -> &'static str {
        "ReadPackage"
    }
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        let principal = EntityTypeName::from_str("Iam::User").expect("Valid principal type");
        let resource = EntityTypeName::from_str("Artifact::Package").expect("Valid resource type");
        (principal, resource)
    }
}

struct WritePackageAction;

impl Action for WritePackageAction {
    fn name() -> &'static str {
        "WritePackage"
    }
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        let principal = EntityTypeName::from_str("Iam::User").expect("Valid principal type");
        let resource = EntityTypeName::from_str("Artifact::Package").expect("Valid resource type");
        (principal, resource)
    }
}

struct ManageRepositoryAction;

impl Action for ManageRepositoryAction {
    fn name() -> &'static str {
        "ManageRepository"
    }
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        let principal = EntityTypeName::from_str("Iam::Group").expect("Valid principal type");
        let resource = EntityTypeName::from_str("Artifact::Repository").expect("Valid resource type");
        (principal, resource)
    }
}

// ============================================================================
// Helper para renderizar schema
// ============================================================================

fn render_schema(schema: &Schema) -> String {
    format!("{:#?}", schema)
}

fn print_schema_details(schema: &Schema, title: &str) {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║ {:<62} ║", title);
    println!("╚════════════════════════════════════════════════════════════════╝");

    let schema_str = render_schema(schema);

    println!("\n📋 Schema Debug Output:");
    println!("{}", schema_str);

    println!("\n✅ Schema built successfully!");
    println!("   - Entity types, actions, and relationships are properly defined");
    println!("   - Namespaces are correctly structured");
    println!("   - All fragments were merged without conflicts\n");
}

/// Valida que una política es válida contra el schema usando Cedar
fn validate_policy_against_schema(schema: &Schema, policy_str: &str) -> Result<(), String> {
    let policy: Policy = policy_str.parse()
        .map_err(|e| format!("Failed to parse policy: {}", e))?;

    let mut policy_set = PolicySet::new();
    policy_set.add(policy)
        .map_err(|e| format!("Failed to add policy to set: {}", e))?;

    let validator = cedar_policy::Validator::new(schema.clone());
    let validation_result = validator.validate(&policy_set, cedar_policy::ValidationMode::default());

    if validation_result.validation_passed() {
        Ok(())
    } else {
        let errors: Vec<String> = validation_result.validation_errors()
            .map(|e| format!("{:?}", e))
            .collect();
        Err(format!("Validation failed: {:?}", errors))
    }
}

/// Verifica que el schema contiene los componentes esperados usando validación de políticas
fn assert_schema_contains_entities_and_actions(schema: &Schema, expected_components: &[&str]) {
    for component in expected_components {
        let test_policy = if component.starts_with("Action::") {
            // Para una acción como "Action::"ReadPackage"", creamos una política que la use
            format!("permit(principal, action == {}, resource);", component)
        } else if component.contains("::") {
            // Para una entidad como "Iam::User", creamos una política que la use en una condición 'is'
            format!("permit(principal, action, resource) when {{ principal is {} }};", component)
        } else {
            // Ignorar componentes no reconocidos
            continue;
        };

        validate_policy_against_schema(schema, &test_policy)
            .unwrap_or_else(|e| panic!("Schema validation failed for component '{}': {}\nGenerated policy: {}", component, e, test_policy));
    }
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_schema_with_single_principal_and_resource() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Single Principal and Resource ===");
    println!("{}", schema_str);
    println!("=================================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &["Iam::User", "Artifact::Package", "Action::\"ReadPackage\""]
    );

    let test_policy = r#"
        permit(
            principal == Iam::User::"alice",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-123"
        );
    "#;

    validate_policy_against_schema(schema, test_policy)
        .expect("Policy should be valid against schema");
}

#[tokio::test]
async fn test_schema_with_multiple_principals() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_principal::<IamGroup>()
        .expect("register IamGroup")
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Multiple Principals ===");
    println!("{}", schema_str);
    println!("========================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &["Iam::User", "Iam::Group", "Artifact::Package", "Action::\"ReadPackage\""]
    );

    let user_policy = r#"
        permit(
            principal == Iam::User::"bob",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-456"
        );
    "#;
    validate_policy_against_schema(schema, user_policy)
        .expect("User policy should be valid");
}

#[tokio::test]
async fn test_schema_with_multiple_resources() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_resource::<ArtifactRepository>()
        .expect("register ArtifactRepository")
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Multiple Resources ===");
    println!("{}", schema_str);
    println!("=======================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &["Iam::User", "Artifact::Package", "Artifact::Repository", "Action::\"ReadPackage\""]
    );

    let package_policy = r#"
        permit(
            principal == Iam::User::"charlie",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-789"
        );
    "#;
    validate_policy_against_schema(schema, package_policy)
        .expect("Package policy should be valid");
}

#[tokio::test]
async fn test_schema_with_multiple_actions() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_principal::<IamGroup>()
        .expect("register IamGroup")
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_resource::<ArtifactRepository>()
        .expect("register ArtifactRepository")
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction")
        .register_action::<WritePackageAction>()
        .expect("register WritePackageAction")
        .register_action::<ManageRepositoryAction>()
        .expect("register ManageRepositoryAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Multiple Actions ===");
    println!("{}", schema_str);
    println!("=====================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &["Iam::User", "Iam::Group", "Artifact::Package", "Artifact::Repository", "Action::\"ReadPackage\"", "Action::\"WritePackage\"", "Action::\"ManageRepository\""]
    );

    let read_policy = r#"
        permit(
            principal == Iam::User::"dave",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-read"
        );
    "#;
    validate_policy_against_schema(schema, read_policy)
        .expect("Read policy should be valid");

    let write_policy = r#"
        permit(
            principal == Iam::User::"eve",
            action == Action::"WritePackage",
            resource == Artifact::Package::"pkg-write"
        );
    "#;
    validate_policy_against_schema(schema, write_policy)
        .expect("Write policy should be valid");

    let manage_policy = r#"
        permit(
            principal == Iam::Group::"admins",
            action == Action::"ManageRepository",
            resource == Artifact::Repository::"repo-main"
        );
    "#;
    validate_policy_against_schema(schema, manage_policy)
        .expect("Manage policy should be valid");
}

#[tokio::test]
async fn test_schema_with_complex_attributes() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_resource::<ArtifactPackage>()      // <-- Recurso que faltaba
        .expect("register ArtifactPackage")
        .register_resource::<ArtifactRepository>()
        .expect("register ArtifactRepository")
        .register_action::<ReadPackageAction>()       // <-- Acción que faltaba
        .expect("register ReadPackageAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Complex Attributes ===");
    println!("{}", schema_str);
    println!("=======================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &["Iam::User", "Artifact::Package", "Artifact::Repository", "Action::\"ReadPackage\""]
    );

    let complex_policy = r#"
        permit(
            principal == Iam::User::"frank",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-complex"
        ) when {
            principal.active == true
        };
    "#;
    validate_policy_against_schema(schema, complex_policy)
        .expect("Complex policy should be valid");
}

#[tokio::test]
async fn test_complete_schema_rendering() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();

    // Registrar todos los principals
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_principal::<IamGroup>()
        .expect("register IamGroup");

    // Registrar todos los resources
    builder
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_resource::<ArtifactRepository>()
        .expect("register ArtifactRepository");

    // Registrar todas las acciones
    builder
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction")
        .register_action::<WritePackageAction>()
        .expect("register WritePackageAction")
        .register_action::<ManageRepositoryAction>()
        .expect("register ManageRepositoryAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;

    print_schema_details(schema, "COMPLETE SCHEMA RENDERING TEST");

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Schema Components Registered:                                ║");
    println!("║  - Principals: IamUser, IamGroup                              ║");
    println!("║  - Resources: ArtifactPackage, ArtifactRepository             ║");
    println!("║  - Actions: ReadPackage, WritePackage, ManageRepository       ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &[
            "Iam::User", "Iam::Group",
            "Artifact::Package", "Artifact::Repository",
            "Action::\"ReadPackage\"", "Action::\"WritePackage\"", "Action::\"ManageRepository\""
        ]
    );

    let policies = vec![
        r#"permit(principal == Iam::User::"admin", action == Action::"WritePackage", resource == Artifact::Package::"critical-pkg");"#,
        r#"permit(principal == Iam::Group::"devops", action == Action::"ManageRepository", resource == Artifact::Repository::"prod-repo");"#,
        r#"permit(principal == Iam::User::"reader", action == Action::"ReadPackage", resource == Artifact::Package::"public-pkg") when { principal.active == true };"#,
    ];

    for (idx, policy_str) in policies.iter().enumerate() {
        validate_policy_against_schema(schema, policy_str)
            .unwrap_or_else(|e| panic!("Policy {} should be valid: {}", idx, e));
    }

    println!("\n✅ All {} policies validated successfully against the complete schema!", policies.len());
}

#[tokio::test]
async fn test_empty_schema() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let builder = EngineBuilder::new();
    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Empty Schema (No Registrations) ===");
    println!("{}", schema_str);
    println!("========================================\n");

    let iam_pattern = Regex::new(r"namespace\s+Iam").expect("Valid regex");
    let artifact_pattern = Regex::new(r"namespace\s+Artifact").expect("Valid regex");

    assert!(!iam_pattern.is_match(&schema_str), "Empty schema should not contain Iam namespace");
    assert!(!artifact_pattern.is_match(&schema_str), "Empty schema should not contain Artifact namespace");

    let invalid_policy = r#"
        permit(
            principal == Iam::User::"test",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"test"
        );
    "#;

    let result = validate_policy_against_schema(schema, invalid_policy);
    assert!(result.is_err(), "Policy should fail validation against empty schema");
    println!("✅ Policy correctly failed validation against empty schema: {:?}", result.err());
}
</file>

<file path="crates/policies/tests/shared_parallel_test.rs">
use policies::shared::application::parallel::{
    build_entities, build_policy_set, evaluate_scenarios_channel, evaluate_until_first, AuthScenario
};

#[tokio::test]
async fn channel_evaluates_multiple_scenarios() {
    let pset = build_policy_set(&vec![
        "permit(principal, action, resource) when { context.mfa == true };".to_string()
    ]).expect("policy set");
    let ents = build_entities(&[]).expect("entities");

    let scenarios = vec![
        AuthScenario { name: "s1".to_string(), principal: "User::\"u\"".to_string(), action: "Action::\"view\"".to_string(), resource: "Resource::\"r\"".to_string(), context: Some(std::iter::once(("mfa".to_string(), serde_json::json!(true))).collect()) },
        AuthScenario { name: "s2".to_string(), principal: "User::\"u\"".to_string(), action: "Action::\"view\"".to_string(), resource: "Resource::\"r\"".to_string(), context: Some(std::iter::once(("mfa".to_string(), serde_json::json!(true))).collect()) },
    ];

    let (outcomes, stats) = evaluate_scenarios_channel(&pset, &ents, scenarios, None, 4, 8).await.expect("run");
    assert_eq!(outcomes.len(), 2);
    assert_eq!(stats.scenarios_total, 2);
    assert!(outcomes.iter().all(|o| o.allow));
}

#[tokio::test]
async fn until_first_returns_on_first_allow() {
    let pset = build_policy_set(&vec![
        "permit(principal, action, resource) when { context.allowed == true };".to_string()
    ]).expect("policy set");
    let ents = build_entities(&[]).expect("entities");

    let mut ctx_deny = std::collections::HashMap::new();
    ctx_deny.insert("allowed".to_string(), serde_json::json!(false));
    let mut ctx_allow = std::collections::HashMap::new();
    ctx_allow.insert("allowed".to_string(), serde_json::json!(true));

    let scenarios = vec![
        AuthScenario { name: "deny".to_string(), principal: "User::\"u\"".to_string(), action: "Action::\"a\"".to_string(), resource: "Resource::\"r\"".to_string(), context: Some(ctx_deny) },
        AuthScenario { name: "allow".to_string(), principal: "User::\"u\"".to_string(), action: "Action::\"a\"".to_string(), resource: "Resource::\"r\"".to_string(), context: Some(ctx_allow) },
    ];

    let first = evaluate_until_first(&pset, &ents, scenarios, None, 2, 4, |o| o.allow).await.expect("run");
    assert!(first.is_some());
    assert_eq!(first.unwrap().name, "allow");
}
</file>

<file path="crates/shared/src/application/ports/mod.rs">
//! Application ports for the shared kernel
//!
//! This module contains the contract definitions (ports) that define
//! the interfaces between the application layer and infrastructure layer.
pub mod event_bus;
pub mod unit_of_work;

// Re-export commonly used types
pub use event_bus::{
    DomainEvent, EventBus, EventEnvelope, EventHandler, EventPublisher, Subscription,
};
pub use unit_of_work::{UnitOfWork, UnitOfWorkError, UnitOfWorkFactory};
</file>

<file path="crates/shared/src/infrastructure/mod.rs">
//! Infrastructure layer for shared services and adapters

pub mod audit;
pub mod in_memory_event_bus;
pub mod surrealdb_adapter;

// Re-export commonly used infrastructure types
pub use audit::{AuditEventHandler, AuditLog, AuditLogStore, AuditStats};
pub use in_memory_event_bus::InMemoryEventBus;
</file>

<file path="crates/shared/src/infrastructure/surrealdb_adapter.rs">
//! SurrealDB infrastructure adapter for shared persistence layer
//!
//! Nota: Este adaptador todavía es una implementación "in-memory / placeholder".
//! Sin embargo, ahora todos los campos previamente marcados como "dead code"
//! (config, connection, table_name) son usados explícitamente para:
//!  - Construir identificadores
//!  - Registrar trazas con `tracing`
//!  - Exponer metadatos de conexión
//!  - Ejecución de tests que validan el comportamiento básico
//!
//! Así eliminamos los warnings de `dead_code` mientras mantenemos la
//! extensibilidad para una futura integración real con SurrealDB.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, instrument};

#[derive(Debug, Error)]
pub enum SurrealDbError {
    #[error("SurrealDB connection error: {0}")]
    ConnectionError(String),

    #[error("SurrealDB query error: {0}")]
    QueryError(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Record not found: {0}")]
    RecordNotFound(String),

    #[error("Invalid record ID: {0}")]
    InvalidRecordId(String),

    #[error("Parse error: {0}")]
    ParseError(String),
}

/// SurrealDB connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurrealDbConfig {
    pub url: String,
    pub namespace: String,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Default for SurrealDbConfig {
    fn default() -> Self {
        Self {
            url: "ws://localhost:8000".to_string(),
            namespace: "hodei".to_string(),
            database: "hodei".to_string(),
            username: None,
            password: None,
        }
    }
}

/// SurrealDB connection manager
#[derive(Debug, Clone)]
pub struct SurrealDbConnection {
    config: SurrealDbConfig,
    // Futuro: aquí iría el cliente real de SurrealDB
}

impl SurrealDbConnection {
    /// Crea una nueva conexión (aún sin abrir físicamente)
    pub fn new(config: SurrealDbConfig) -> Result<Self, SurrealDbError> {
        debug!(
            url = %config.url,
            ns = %config.namespace,
            db = %config.database,
            "Initializing SurrealDbConnection"
        );
        Ok(Self { config })
    }

    /// Establece la conexión (placeholder)
    #[instrument(level = "debug", skip(self))]
    pub async fn connect(&self) -> Result<(), SurrealDbError> {
        // Uso explícito de los campos para evitar dead_code
        debug!(
            url = %self.config.url,
            ns = %self.config.namespace,
            db = %self.config.database,
            "Connecting to SurrealDB (placeholder)"
        );
        Ok(())
    }

    /// Cierra la conexión (placeholder)
    #[instrument(level = "debug", skip(self))]
    pub async fn disconnect(&self) -> Result<(), SurrealDbError> {
        debug!(
            url = %self.config.url,
            ns = %self.config.namespace,
            db = %self.config.database,
            "Disconnecting from SurrealDB (placeholder)"
        );
        Ok(())
    }

    /// Devuelve el namespace configurado
    pub fn namespace(&self) -> &str {
        &self.config.namespace
    }

    /// Devuelve el nombre de la base de datos configurada
    pub fn database(&self) -> &str {
        &self.config.database
    }

    /// Devuelve la URL de conexión
    pub fn url(&self) -> &str {
        &self.config.url
    }

    /// Devuelve si la conexión tiene credenciales embebidas
    pub fn has_credentials(&self) -> bool {
        self.config.username.is_some() && self.config.password.is_some()
    }

    /// Exponer (lectura) la configuración completa - útil para adaptadores superiores
    pub fn config(&self) -> &SurrealDbConfig {
        &self.config
    }
}

/// SurrealDB repository trait for common operations
#[async_trait::async_trait]
pub trait SurrealDbRepository<T>: Send + Sync
where
    T: Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
    async fn create(&self, record: T) -> Result<String, SurrealDbError>;
    async fn read(&self, id: &str) -> Result<Option<T>, SurrealDbError>;
    async fn update(&self, id: &str, record: T) -> Result<(), SurrealDbError>;
    async fn delete(&self, id: &str) -> Result<(), SurrealDbError>;
    async fn list(&self, limit: Option<u64>) -> Result<Vec<T>, SurrealDbError>;
}

/// Generic SurrealDB repository implementation
///
/// Conserva *state* (connection + table_name) que ahora se usa en todas las operaciones
pub struct SurrealDbGenericRepository<T> {
    connection: Arc<SurrealDbConnection>,
    table_name: String,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> SurrealDbGenericRepository<T> {
    pub fn new(connection: Arc<SurrealDbConnection>, table_name: &str) -> Self {
        debug!(
            table = %table_name,
            db = %connection.database(),
            ns = %connection.namespace(),
            "Creating SurrealDbGenericRepository"
        );
        Self {
            connection,
            table_name: table_name.to_string(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Devuelve el nombre de la tabla objetivo
    pub fn table(&self) -> &str {
        &self.table_name
    }

    /// Devuelve referencia de la conexión (para adaptadores externos)
    pub fn connection(&self) -> &Arc<SurrealDbConnection> {
        &self.connection
    }
}

#[async_trait::async_trait]
impl<T> SurrealDbRepository<T> for SurrealDbGenericRepository<T>
where
    T: Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
    #[instrument(level = "debug", skip(self, _record))]
    async fn create(&self, _record: T) -> Result<String, SurrealDbError> {
        // Usamos table_name y metadata de connection para generar un ID
        let id = utils::generate_record_id(&self.table_name, "rec_");
        debug!(
            table = %self.table_name,
            db = %self.connection.database(),
            ns = %self.connection.namespace(),
            %id,
            "Create (placeholder)"
        );
        Ok(id)
    }

    #[instrument(level = "debug", skip(self))]
    async fn read(&self, id: &str) -> Result<Option<T>, SurrealDbError> {
        debug!(
            table = %self.table_name,
            db = %self.connection.database(),
            ns = %self.connection.namespace(),
            %id,
            "Read (placeholder)"
        );
        Ok(None)
    }

    #[instrument(level = "debug", skip(self, _record))]
    async fn update(&self, id: &str, _record: T) -> Result<(), SurrealDbError> {
        debug!(
            table = %self.table_name,
            db = %self.connection.database(),
            ns = %self.connection.namespace(),
            %id,
            "Update (placeholder)"
        );
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn delete(&self, id: &str) -> Result<(), SurrealDbError> {
        debug!(
            table = %self.table_name,
            db = %self.connection.database(),
            ns = %self.connection.namespace(),
            %id,
            "Delete (placeholder)"
        );
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn list(&self, limit: Option<u64>) -> Result<Vec<T>, SurrealDbError> {
        debug!(
            table = %self.table_name,
            db = %self.connection.database(),
            ns = %self.connection.namespace(),
            ?limit,
            "List (placeholder)"
        );
        Ok(Vec::new())
    }
}

/// SurrealDB query builder for future complex queries
pub struct SurrealDbQueryBuilder {
    query: String,
    params: Vec<String>,
}

impl SurrealDbQueryBuilder {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            params: Vec::new(),
        }
    }

    pub fn select(mut self, table: &str) -> Self {
        self.query = format!("SELECT * FROM {}", table);
        self
    }

    pub fn where_clause(mut self, condition: &str) -> Self {
        self.query = format!("{} WHERE {}", self.query, condition);
        self
    }

    pub fn order_by(mut self, field: &str, direction: &str) -> Self {
        self.query = format!("{} ORDER BY {} {}", self.query, field, direction);
        self
    }

    pub fn limit(mut self, limit: u64) -> Self {
        self.query = format!("{} LIMIT {}", self.query, limit);
        self
    }

    pub fn build(self) -> (String, Vec<String>) {
        (self.query, self.params)
    }
}

impl Default for SurrealDbQueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Common utilities for SurrealDB operations
pub mod utils {
    use super::*;
    use time::OffsetDateTime;

    /// Generate a SurrealDB record ID (format: table:prefix<timestamp>)
    pub fn generate_record_id(table: &str, prefix: &str) -> String {
        let timestamp = OffsetDateTime::now_utc().unix_timestamp();
        format!("{}:{}{}", table, prefix, timestamp)
    }

    /// Convert OffsetDateTime to SurrealDB datetime format
    pub fn to_surreal_datetime(dt: OffsetDateTime) -> String {
        dt.format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| String::from(""))
    }

    /// Parse SurrealDB datetime string to OffsetDateTime
    pub fn from_surreal_datetime(dt_str: &str) -> Result<OffsetDateTime, SurrealDbError> {
        OffsetDateTime::parse(dt_str, &time::format_description::well_known::Rfc3339)
            .map_err(|e| SurrealDbError::ParseError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use time::OffsetDateTime;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestRecord {
        id: String,
        name: String,
        created_at: String,
    }

    #[tokio::test]
    async fn test_connection_and_repository_usage() {
        let _ = tracing_subscriber::fmt::try_init();

        let config = SurrealDbConfig::default();
        let conn = Arc::new(SurrealDbConnection::new(config.clone()).unwrap());
        conn.connect().await.unwrap();

        assert_eq!(conn.database(), &config.database);
        assert_eq!(conn.namespace(), &config.namespace);
        assert_eq!(conn.url(), &config.url);
        assert!(!conn.has_credentials());

        let repo: SurrealDbGenericRepository<TestRecord> =
            SurrealDbGenericRepository::new(conn.clone(), "test_records");

        assert_eq!(repo.table(), "test_records");
        assert_eq!(repo.connection().database(), "hodei");

        // create -> ensures table_name + connection fields were "read"
        let id = repo
            .create(TestRecord {
                id: "temp".into(),
                name: "example".into(),
                created_at: "now".into(),
            })
            .await
            .unwrap();

        assert!(id.starts_with("test_records:rec_"));

        // list placeholder
        let list = repo.list(Some(10)).await.unwrap();
        assert!(list.is_empty());

        conn.disconnect().await.unwrap();
    }

    #[test]
    fn test_config_default() {
        let config = SurrealDbConfig::default();
        assert_eq!(config.url, "ws://localhost:8000");
        assert_eq!(config.namespace, "hodei");
        assert_eq!(config.database, "hodei");
    }

    #[test]
    fn test_query_builder() {
        let (query, _params) = SurrealDbQueryBuilder::new()
            .select("users")
            .where_clause("age > 18")
            .order_by("created_at", "DESC")
            .limit(10)
            .build();

        assert!(query.contains("SELECT * FROM users"));
        assert!(query.contains("WHERE age > 18"));
        assert!(query.contains("ORDER BY created_at DESC"));
        assert!(query.contains("LIMIT 10"));
    }

    #[test]
    fn test_utils() {
        let record_id = utils::generate_record_id("test", "prefix_");
        assert!(record_id.starts_with("test:prefix_"));

        let dt = OffsetDateTime::now_utc();
        let dt_str = utils::to_surreal_datetime(dt);
        assert!(!dt_str.is_empty());

        let parsed = utils::from_surreal_datetime(&dt_str).unwrap();
        assert!(parsed.unix_timestamp() <= dt.unix_timestamp() + 1);
    }
}
</file>

<file path="crates/hodei-authorizer/src/features/evaluate_permissions/mocks.rs">
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use crate::features::evaluate_permissions::dto::{
    AuthorizationDecision, AuthorizationRequest, AuthorizationResponse,
};
use crate::features::evaluate_permissions::error::EvaluatePermissionsResult;
use crate::features::evaluate_permissions::ports::{
    AuthorizationCache, AuthorizationLogger, AuthorizationMetrics,
};
use policies::shared::domain::hrn::Hrn;

/// Mock Authorization Cache for testing
#[derive(Debug, Default, Clone)]
pub struct MockAuthorizationCache {
    responses: Arc<Mutex<std::collections::HashMap<String, AuthorizationResponse>>>,
}

impl MockAuthorizationCache {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    pub fn with_response(self, cache_key: &str, response: AuthorizationResponse) -> Self {
        let mut responses = self.responses.lock().unwrap();
        responses.insert(cache_key.to_string(), response);
        drop(responses);
        self
    }
}

#[async_trait]
impl AuthorizationCache for MockAuthorizationCache {
    async fn get(
        &self,
        cache_key: &str,
    ) -> EvaluatePermissionsResult<Option<AuthorizationResponse>> {
        let responses = self.responses.lock().unwrap();
        Ok(responses.get(cache_key).cloned())
    }

    async fn put(
        &self,
        cache_key: &str,
        response: &AuthorizationResponse,
        _ttl: std::time::Duration,
    ) -> EvaluatePermissionsResult<()> {
        let mut responses = self.responses.lock().unwrap();
        responses.insert(cache_key.to_string(), response.clone());
        Ok(())
    }

    async fn invalidate_principal(&self, _principal_hrn: &Hrn) -> EvaluatePermissionsResult<()> {
        Ok(())
    }

    async fn invalidate_resource(&self, _resource_hrn: &Hrn) -> EvaluatePermissionsResult<()> {
        Ok(())
    }
}

/// Mock Authorization Logger for testing
#[derive(Debug, Default, Clone)]
pub struct MockAuthorizationLogger {
    decisions_logged: Arc<Mutex<Vec<AuthorizationResponse>>>,
}

impl MockAuthorizationLogger {
    pub fn new() -> Self {
        Self {
            decisions_logged: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn get_logged_decisions(&self) -> Vec<AuthorizationResponse> {
        let logged = self.decisions_logged.lock().unwrap();
        logged.clone()
    }
}

#[async_trait]
impl AuthorizationLogger for MockAuthorizationLogger {
    async fn log_decision(
        &self,
        _request: &AuthorizationRequest,
        response: &AuthorizationResponse,
    ) -> EvaluatePermissionsResult<()> {
        let mut logged = self.decisions_logged.lock().unwrap();
        logged.push(response.clone());
        Ok(())
    }

    async fn log_error(
        &self,
        _request: &AuthorizationRequest,
        _error: &crate::features::evaluate_permissions::error::EvaluatePermissionsError,
    ) -> EvaluatePermissionsResult<()> {
        Ok(())
    }
}

/// Mock Authorization Metrics for testing
#[derive(Debug, Default, Clone)]
pub struct MockAuthorizationMetrics {
    decisions_recorded: Arc<Mutex<Vec<AuthorizationDecision>>>,
}

impl MockAuthorizationMetrics {
    pub fn new() -> Self {
        Self {
            decisions_recorded: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn get_recorded_decisions(&self) -> Vec<AuthorizationDecision> {
        let recorded = self.decisions_recorded.lock().unwrap();
        recorded.clone()
    }
}

#[async_trait]
impl AuthorizationMetrics for MockAuthorizationMetrics {
    async fn record_decision(
        &self,
        decision: &AuthorizationDecision,
        _evaluation_time_ms: u64,
    ) -> EvaluatePermissionsResult<()> {
        let mut recorded = self.decisions_recorded.lock().unwrap();
        recorded.push(decision.clone());
        Ok(())
    }

    async fn record_error(&self, _error_type: &str) -> EvaluatePermissionsResult<()> {
        Ok(())
    }

    async fn record_cache_hit(&self, _hit: bool) -> EvaluatePermissionsResult<()> {
        Ok(())
    }
}

/// Mock SCP Repository for testing
#[derive(Debug, Clone)]
pub struct MockScpRepository;

/// Mock Org Repository for testing
#[derive(Debug, Clone)]
pub struct MockOrgRepository;

/// Helper functions for creating test data
pub mod test_helpers {
    use super::*;
    use crate::features::evaluate_permissions::dto::AuthorizationContext;

    /// Create a test HRN
    pub fn create_test_hrn(resource_type: &str, resource_id: &str) -> Hrn {
        Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "us-east-1".to_string(),
            resource_type.to_string(),
            resource_id.to_string(),
        )
    }

    /// Create a test authorization request
    pub fn create_test_request(
        principal_hrn: Hrn,
        action: String,
        resource_hrn: Hrn,
    ) -> AuthorizationRequest {
        AuthorizationRequest {
            principal: principal_hrn,
            action,
            resource: resource_hrn,
            context: None,
        }
    }

    /// Create a test authorization request with context
    pub fn create_test_request_with_context(
        principal_hrn: Hrn,
        action: String,
        resource_hrn: Hrn,
        context: AuthorizationContext,
    ) -> AuthorizationRequest {
        AuthorizationRequest {
            principal: principal_hrn,
            action,
            resource: resource_hrn,
            context: Some(context),
        }
    }
}
</file>

<file path="crates/hodei-authorizer/src/features/evaluate_permissions/use_case.rs">
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, instrument, warn};

use crate::features::evaluate_permissions::dto::{
    AuthorizationDecision, AuthorizationRequest, AuthorizationResponse,
};
use crate::features::evaluate_permissions::error::{
    EvaluatePermissionsError, EvaluatePermissionsResult,
};
use crate::features::evaluate_permissions::ports::{
    AuthorizationCache, AuthorizationLogger, AuthorizationMetrics,
};
use policies::shared::AuthorizationEngine;

// Importar casos de uso de otros crates (NO entidades internas)
use hodei_iam::{DynEffectivePoliciesQueryService, GetEffectivePoliciesQuery};
use hodei_organizations::GetEffectiveScpsQuery;

/// Use case for evaluating authorization permissions with multi-layer security
///
/// Esta implementación sigue el principio de responsabilidad única:
/// - NO gestiona políticas directamente
/// - USA casos de uso de otros crates para obtener políticas
/// - DELEGA la evaluación al AuthorizationEngine de policies
/// - Gestiona aspectos transversales: cache, logging, metrics
pub struct EvaluatePermissionsUseCase<CACHE, LOGGER, METRICS> {
    // ✅ Casos de uso de otros crates (NO providers custom)
    iam_use_case: DynEffectivePoliciesQueryService,
    org_use_case: Option<Arc<dyn GetEffectiveScpsPort>>,

    // ✅ Motor de autorización del crate policies
    authorization_engine: Arc<AuthorizationEngine>,

    // ✅ Aspectos transversales
    cache: Option<CACHE>,
    logger: LOGGER,
    metrics: METRICS,
}

/// Trait para abstraer el caso de uso de SCPs efectivas
#[async_trait::async_trait]
pub trait GetEffectiveScpsPort: Send + Sync {
    async fn execute(
        &self,
        query: GetEffectiveScpsQuery,
    ) -> Result<cedar_policy::PolicySet, Box<dyn std::error::Error + Send + Sync>>;
}

impl<CACHE, LOGGER, METRICS> EvaluatePermissionsUseCase<CACHE, LOGGER, METRICS>
where
    CACHE: AuthorizationCache,
    LOGGER: AuthorizationLogger,
    METRICS: AuthorizationMetrics,
{
    /// Create a new instance of the use case
    pub fn new(
        iam_use_case: DynEffectivePoliciesQueryService,
        org_use_case: Option<Arc<dyn GetEffectiveScpsPort>>,
        authorization_engine: Arc<AuthorizationEngine>,
        cache: Option<CACHE>,
        logger: LOGGER,
        metrics: METRICS,
    ) -> Self {
        Self {
            iam_use_case,
            org_use_case,
            authorization_engine,
            cache,
            logger,
            metrics,
        }
    }

    /// Evaluate authorization request with multi-layer security
    #[instrument(skip(self), fields(principal = %request.principal, resource = %request.resource, action = %request.action))]
    pub async fn execute(
        &self,
        request: AuthorizationRequest,
    ) -> EvaluatePermissionsResult<AuthorizationResponse> {
        let start_time = Instant::now();

        // Generate cache key and check cache first
        let cache_key = self.generate_cache_key(&request);
        if let Some(ref cache) = self.cache {
            if let Ok(Some(cached_response)) = cache.get(&cache_key).await {
                info!("Authorization decision served from cache");
                self.metrics.record_cache_hit(true).await?;
                return Ok(cached_response);
            }
            self.metrics.record_cache_hit(false).await?;
        }

        // Execute the evaluation
        let result = self.evaluate_authorization(&request).await;
        let evaluation_time_ms = start_time.elapsed().as_millis() as u64;

        // Log and record metrics
        match &result {
            Ok(response) => {
                self.logger.log_decision(&request, response).await?;
                self.metrics
                    .record_decision(&response.decision, evaluation_time_ms)
                    .await?;
            }
            Err(error) => {
                self.logger.log_error(&request, error).await?;
                self.metrics
                    .record_error(std::any::type_name_of_val(error))
                    .await?;
            }
        }

        // Cache the result if successful
        if let (Ok(response), Some(cache)) = (&result, &self.cache) {
            let ttl = std::time::Duration::from_secs(300); // 5 minutes cache
            if let Err(cache_error) = cache.put(&cache_key, response, ttl).await {
                warn!("Failed to cache authorization decision: {}", cache_error);
            }
        }

        result
    }

    /// Core authorization evaluation logic - orchestrates policy collection and delegates to AuthorizationEngine
    async fn evaluate_authorization(
        &self,
        request: &AuthorizationRequest,
    ) -> EvaluatePermissionsResult<AuthorizationResponse> {
        info!("Starting multi-layer authorization evaluation (orchestration)");

        // Step 1: Get IAM policies using hodei-iam use case
        info!("Fetching IAM policies for principal");
        let iam_query = GetEffectivePoliciesQuery {
            principal_hrn: request.principal.to_string(),
        };

        let iam_response = self
            .iam_use_case
            .get_effective_policies(iam_query)
            .await
            .map_err(|e| {
                EvaluatePermissionsError::IamPolicyProviderError(format!(
                    "Failed to get IAM policies: {}",
                    e
                ))
            })?;

        info!(
            "Retrieved {} IAM policies for principal",
            iam_response.policy_count
        );

        // Step 2: Get SCPs using hodei-organizations use case (if available)
        let scp_policy_set = if let Some(ref org_use_case) = self.org_use_case {
            info!("Fetching effective SCPs for resource");
            let scp_query = GetEffectiveScpsQuery {
                resource_hrn: request.resource.to_string(),
            };

            let scp_response = org_use_case.execute(scp_query).await.map_err(|e| {
                EvaluatePermissionsError::OrganizationBoundaryProviderError(format!(
                    "Failed to get SCPs: {}",
                    e
                ))
            })?;

            info!(
                "Retrieved {} SCPs for resource",
                scp_response.policies().count()
            );
            scp_response
        } else {
            info!("No organization use case configured, skipping SCPs");
            cedar_policy::PolicySet::new()
        };

        // Step 3: Combine PolicySets
        info!("Combining IAM policies and SCPs");
        let mut combined_policies = cedar_policy::PolicySet::new();

        // Add SCPs first (higher precedence in evaluation - deny overrides)
        for scp_policy in scp_policy_set.policies() {
            if let Err(e) = combined_policies.add(scp_policy.clone()) {
                warn!("Failed to add SCP policy: {}", e);
                // Continue with other policies even if one fails
            }
        }

        // Add IAM policies
        for iam_policy in iam_response.policies.policies() {
            if let Err(e) = combined_policies.add(iam_policy.clone()) {
                warn!("Failed to add IAM policy: {}", e);
                // Continue with other policies even if one fails
            }
        }

        info!(
            "Combined {} total policies, delegating evaluation to AuthorizationEngine",
            combined_policies.policies().count()
        );

        // Step 4: Delegate evaluation to policies crate's AuthorizationEngine
        let decision = self
            .evaluate_with_policy_set(request, &combined_policies)
            .await?;

        info!(
            "Authorization evaluation completed: {:?}",
            decision.decision
        );
        Ok(decision)
    }

    /// Evaluate authorization by delegating to the policies crate's AuthorizationEngine
    /// This method will be enhanced once we have proper HodeiEntity implementations
    async fn evaluate_with_policy_set(
        &self,
        request: &AuthorizationRequest,
        policies: &cedar_policy::PolicySet,
    ) -> EvaluatePermissionsResult<AuthorizationResponse> {
        use cedar_policy::EntityUid;
        use std::str::FromStr;

        // If no policies, apply Principle of Least Privilege (implicit deny)
        if policies.is_empty() {
            info!("No policies found - applying Principle of Least Privilege (implicit deny)");
            return Ok(AuthorizationResponse::implicit_deny(
                "No policies matched - access denied by Principle of Least Privilege".to_string(),
            ));
        }

        // Convert request to Cedar format
        let principal = EntityUid::from_str(&request.principal.to_string()).map_err(|e| {
            EvaluatePermissionsError::InvalidRequest(format!("Invalid principal HRN: {}", e))
        })?;

        let action =
            EntityUid::from_str(&format!("Action::\"{}\"", request.action)).map_err(|e| {
                EvaluatePermissionsError::InvalidRequest(format!("Invalid action: {}", e))
            })?;

        let resource = EntityUid::from_str(&request.resource.to_string()).map_err(|e| {
            EvaluatePermissionsError::InvalidRequest(format!("Invalid resource HRN: {}", e))
        })?;

        let context = self.create_cedar_context(request)?;

        // Create authorization request for policies crate
        // Note: Using empty entities vector for now - will be enhanced with proper entity resolution
        let auth_request = policies::shared::AuthorizationRequest {
            principal: &MockHodeiEntity {
                euid: principal.clone(),
                mock_hrn: policies::shared::Hrn::from_string("hrn:hodei:iam::principal/mock")
                    .unwrap(),
            },
            action: action.clone(),
            resource: &MockHodeiEntity {
                euid: resource.clone(),
                mock_hrn: policies::shared::Hrn::from_string("hrn:hodei:resource::mock/resource")
                    .unwrap(),
            },
            context,
            entities: vec![], // Will be populated with actual entities later
        };

        // Delegate to policies crate's AuthorizationEngine with the combined PolicySet
        info!("Delegating evaluation to policies::AuthorizationEngine");
        let response = self
            .authorization_engine
            .is_authorized_with_policy_set(&auth_request, policies);

        // Convert Cedar response to our DTO
        let (decision, determining_policies, explicit, reason) = match response.decision() {
            cedar_policy::Decision::Deny => {
                let policies: Vec<String> = response
                    .diagnostics()
                    .reason()
                    .map(|p| p.to_string())
                    .collect();
                (
                    AuthorizationDecision::Deny,
                    policies,
                    true,
                    "Access explicitly denied by policy".to_string(),
                )
            }
            cedar_policy::Decision::Allow => {
                let policies: Vec<String> = response
                    .diagnostics()
                    .reason()
                    .map(|p| p.to_string())
                    .collect();
                (
                    AuthorizationDecision::Allow,
                    policies,
                    true,
                    "Access explicitly allowed by policy".to_string(),
                )
            }
        };

        Ok(AuthorizationResponse {
            decision,
            determining_policies,
            reason,
            explicit,
        })
    }

    /// Create Cedar context from request context
    fn create_cedar_context(
        &self,
        request: &AuthorizationRequest,
    ) -> EvaluatePermissionsResult<cedar_policy::Context> {
        use time::format_description::well_known::Rfc3339;

        let mut context_data = serde_json::Map::new();

        if let Some(ref request_context) = request.context {
            if let Some(ref source_ip) = request_context.source_ip {
                context_data.insert(
                    "source_ip".to_string(),
                    serde_json::Value::String(source_ip.clone()),
                );
            }
            if let Some(ref user_agent) = request_context.user_agent {
                context_data.insert(
                    "user_agent".to_string(),
                    serde_json::Value::String(user_agent.clone()),
                );
            }
            if let Some(ref request_time) = request_context.request_time
                && let Ok(formatted) = request_time.format(&Rfc3339)
            {
                context_data.insert(
                    "request_time".to_string(),
                    serde_json::Value::String(formatted),
                );
            }

            // Add additional context
            for (key, value) in &request_context.additional_context {
                context_data.insert(key.clone(), value.clone());
            }
        }

        let context_json = serde_json::Value::Object(context_data);
        cedar_policy::Context::from_json_value(context_json, None).map_err(|e| {
            EvaluatePermissionsError::InvalidRequest(format!("Invalid context: {}", e))
        })
    }

    /// Generate cache key for authorization request
    fn generate_cache_key(&self, request: &AuthorizationRequest) -> String {
        format!(
            "auth:{}:{}:{}",
            request.principal, request.action, request.resource
        )
    }
}

/// Temporary mock implementation of HodeiEntity for authorization requests
/// This will be replaced with proper entity implementations from hodei-iam and hodei-organizations
struct MockHodeiEntity {
    euid: cedar_policy::EntityUid,
    mock_hrn: policies::shared::Hrn,
}

impl policies::domain::HodeiEntity for MockHodeiEntity {
    fn hrn(&self) -> &policies::shared::Hrn {
        &self.mock_hrn
    }
    fn euid(&self) -> cedar_policy::EntityUid {
        self.euid.clone()
    }

    fn attributes(&self) -> std::collections::HashMap<String, cedar_policy::RestrictedExpression> {
        std::collections::HashMap::new()
    }

    fn parents(&self) -> Vec<cedar_policy::EntityUid> {
        vec![]
    }
}
</file>

<file path="crates/hodei-iam/src/features/add_user_to_group/di.rs">
use super::use_case::AddUserToGroupUseCase;
use crate::shared::application::ports::{GroupRepository, UserRepository};
/// Dependency Injection for add_user_to_group feature
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

pub fn make_use_case(
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
) -> AddUserToGroupUseCase {
    AddUserToGroupUseCase::new(user_repo, group_repo)
}

pub fn make_use_case_with_events(
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
    event_bus: Arc<InMemoryEventBus>,
) -> AddUserToGroupUseCase {
    AddUserToGroupUseCase::new(user_repo, group_repo).with_event_publisher(event_bus)
}
</file>

<file path="crates/hodei-iam/src/features/add_user_to_group/use_case.rs">
use super::dto::AddUserToGroupCommand;
use crate::shared::{
    application::ports::{GroupRepository, UserRepository},
    domain::events::UserAddedToGroup,
};
use policies::shared::domain::hrn::Hrn;
use shared::EventPublisher;
use shared::application::ports::event_bus::EventEnvelope;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
/// Use case for adding a user to a group
use std::sync::Arc;

pub struct AddUserToGroupUseCase {
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl AddUserToGroupUseCase {
    pub fn new(user_repo: Arc<dyn UserRepository>, group_repo: Arc<dyn GroupRepository>) -> Self {
        Self {
            user_repo,
            group_repo,
            event_publisher: None,
        }
    }

    pub fn with_event_publisher(mut self, publisher: Arc<InMemoryEventBus>) -> Self {
        self.event_publisher = Some(publisher);
        self
    }

    pub async fn execute(&self, cmd: AddUserToGroupCommand) -> Result<(), anyhow::Error> {
        // Parse HRNs
        let user_hrn = Hrn::from_string(&cmd.user_hrn)
            .ok_or_else(|| anyhow::anyhow!("Invalid user HRN: {}", cmd.user_hrn))?;
        let group_hrn = Hrn::from_string(&cmd.group_hrn)
            .ok_or_else(|| anyhow::anyhow!("Invalid group HRN: {}", cmd.group_hrn))?;

        // Validate that the group exists to maintain consistency
        if self.group_repo.find_by_hrn(&group_hrn).await?.is_none() {
            return Err(anyhow::anyhow!("Group not found: {}", cmd.group_hrn));
        }

        // Load the user
        let mut user = self
            .user_repo
            .find_by_hrn(&user_hrn)
            .await?
            .ok_or_else(|| anyhow::anyhow!("User not found: {}", cmd.user_hrn))?;

        // Add user to group (domain logic handles idempotency)
        user.add_to_group(group_hrn.clone());

        // Persist the updated user
        self.user_repo.save(&user).await?;

        // Publish domain event
        if let Some(publisher) = &self.event_publisher {
            let event = UserAddedToGroup {
                user_hrn: user_hrn.clone(),
                group_hrn: group_hrn.clone(),
                added_at: chrono::Utc::now(),
            };

            let envelope = EventEnvelope::new(event)
                .with_metadata("aggregate_type".to_string(), "Group".to_string());

            if let Err(e) = publisher.publish_with_envelope(envelope).await {
                tracing::warn!("Failed to publish UserAddedToGroup event: {}", e);
                // Don't fail the use case if event publishing fails
            }
        }

        Ok(())
    }
}
</file>

<file path="crates/hodei-iam/src/features/create_group/di.rs">
use super::use_case::CreateGroupUseCase;
use crate::shared::application::ports::GroupRepository;
/// Dependency Injection for create_group feature
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

pub fn make_use_case(repo: Arc<dyn GroupRepository>) -> CreateGroupUseCase {
    CreateGroupUseCase::new(repo)
}

pub fn make_use_case_with_events(
    repo: Arc<dyn GroupRepository>,
    event_bus: Arc<InMemoryEventBus>,
) -> CreateGroupUseCase {
    CreateGroupUseCase::new(repo).with_event_publisher(event_bus)
}
</file>

<file path="crates/hodei-iam/src/features/create_group/use_case.rs">
use super::dto::{CreateGroupCommand, GroupView};
use crate::shared::{
    application::ports::GroupRepository,
    domain::{Group, events::GroupCreated},
};
use policies::shared::domain::hrn::Hrn;
use shared::EventPublisher;
use shared::application::ports::event_bus::EventEnvelope;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
/// Use case for creating a new group
use std::sync::Arc;

pub struct CreateGroupUseCase {
    repo: Arc<dyn GroupRepository>,
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl CreateGroupUseCase {
    pub fn new(repo: Arc<dyn GroupRepository>) -> Self {
        Self {
            repo,
            event_publisher: None,
        }
    }

    pub fn with_event_publisher(mut self, publisher: Arc<InMemoryEventBus>) -> Self {
        self.event_publisher = Some(publisher);
        self
    }

    pub async fn execute(&self, cmd: CreateGroupCommand) -> Result<GroupView, anyhow::Error> {
        // Generate a unique HRN using the type-safe constructor
        let group_id = uuid::Uuid::new_v4().to_string();
        let hrn =
            Hrn::for_entity_type::<Group>("hodei".to_string(), "default".to_string(), group_id);

        // Create the group domain entity
        let mut group = Group::new(hrn, cmd.group_name.clone());
        group.tags = cmd.tags.clone();

        // Persist the group
        self.repo.save(&group).await?;

        // Publish domain event
        if let Some(publisher) = &self.event_publisher {
            let event = GroupCreated {
                group_hrn: group.hrn.clone(),
                name: group.name.clone(),
                created_at: chrono::Utc::now(),
            };

            let envelope = EventEnvelope::new(event)
                .with_metadata("aggregate_type".to_string(), "Group".to_string());

            if let Err(e) = publisher.publish_with_envelope(envelope).await {
                tracing::warn!("Failed to publish GroupCreated event: {}", e);
                // Don't fail the use case if event publishing fails
            }
        }

        // Return the view
        Ok(GroupView {
            hrn: group.hrn.to_string(),
            name: group.name,
            tags: group.tags,
        })
    }
}
</file>

<file path="crates/hodei-iam/src/features/create_user/di.rs">
use super::use_case::CreateUserUseCase;
use crate::shared::application::ports::UserRepository;
/// Dependency Injection for create_user feature
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

pub fn make_use_case(repo: Arc<dyn UserRepository>) -> CreateUserUseCase {
    CreateUserUseCase::new(repo)
}

pub fn make_use_case_with_events(
    repo: Arc<dyn UserRepository>,
    event_bus: Arc<InMemoryEventBus>,
) -> CreateUserUseCase {
    CreateUserUseCase::new(repo).with_event_publisher(event_bus)
}
</file>

<file path="crates/hodei-iam/src/features/create_user/use_case.rs">
use super::dto::{CreateUserCommand, UserView};
use crate::shared::{
    application::ports::UserRepository,
    domain::{User, events::UserCreated},
};
use policies::shared::domain::hrn::Hrn;
use shared::EventPublisher;
use shared::application::ports::event_bus::EventEnvelope;
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
/// Use case for creating a new user
use std::sync::Arc;

pub struct CreateUserUseCase {
    repo: Arc<dyn UserRepository>,
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl CreateUserUseCase {
    pub fn new(repo: Arc<dyn UserRepository>) -> Self {
        Self {
            repo,
            event_publisher: None,
        }
    }

    pub fn with_event_publisher(mut self, publisher: Arc<InMemoryEventBus>) -> Self {
        self.event_publisher = Some(publisher);
        self
    }

    pub async fn execute(&self, cmd: CreateUserCommand) -> Result<UserView, anyhow::Error> {
        // Generate a unique HRN using the type-safe constructor
        let user_id = uuid::Uuid::new_v4().to_string();
        let hrn = Hrn::for_entity_type::<User>("hodei".to_string(), "default".to_string(), user_id);

        // Create the user domain entity
        let mut user = User::new(hrn, cmd.name.clone(), cmd.email.clone());
        user.tags = cmd.tags.clone();

        // Persist the user
        self.repo.save(&user).await?;

        // Publish domain event
        if let Some(publisher) = &self.event_publisher {
            let event = UserCreated {
                user_hrn: user.hrn.clone(),
                username: user.name.clone(),
                email: user.email.clone(),
                created_at: chrono::Utc::now(),
            };

            let envelope = EventEnvelope::new(event)
                .with_metadata("aggregate_type".to_string(), "User".to_string());

            if let Err(e) = publisher.publish_with_envelope(envelope).await {
                tracing::warn!("Failed to publish UserCreated event: {}", e);
                // Don't fail the use case if event publishing fails
            }
        }

        // Return the view
        Ok(UserView {
            hrn: user.hrn.to_string(),
            name: user.name,
            email: user.email,
            groups: Vec::new(),
            tags: user.tags,
        })
    }
}
</file>

<file path="crates/hodei-iam/src/shared/domain/mod.rs">
pub mod actions;
/// Domain layer for hodei-iam
pub mod entities;
pub mod events;

pub use actions::{CreateGroupAction, CreateUserAction};
// Re-export for convenience
pub use entities::{Group, Namespace, ServiceAccount, User};
pub use events::*;
</file>

<file path="crates/hodei-iam/tests/integration_create_user_comprehensive_test.rs">
/// Comprehensive integration tests for create_user feature

use hodei_iam::{
    features::create_user::{self, dto::*},
    shared::{
        application::ports::UserRepository,
        infrastructure::persistence::InMemoryUserRepository,
    },
};
use std::sync::Arc;


#[tokio::test]
async fn test_create_user_with_valid_email() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "John Doe".to_string(),
        email: "john.doe@example.com".to_string(),
        tags: vec!["admin".to_string()],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.name, "John Doe");
    assert_eq!(view.email, "john.doe@example.com");
    assert_eq!(view.groups.len(), 0);
    assert_eq!(view.tags.len(), 1);
}

#[tokio::test]
async fn test_create_user_multiple_tags() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Jane Smith".to_string(),
        email: "jane@example.com".to_string(),
        tags: vec!["developer".to_string(), "senior".to_string(), "fullstack".to_string()],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.tags.len(), 3);
    assert!(view.tags.contains(&"developer".to_string()));
    assert!(view.tags.contains(&"senior".to_string()));
}

#[tokio::test]
async fn test_create_user_no_tags() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Bob".to_string(),
        email: "bob@example.com".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());

    let view = result.unwrap();
    assert_eq!(view.tags.len(), 0);
}

#[tokio::test]
async fn test_create_user_hrn_format() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Test User".to_string(),
        email: "test@example.com".to_string(),
        tags: vec![],
    };

    let result = use_case.execute(command).await.unwrap();

    // Verify HRN format: hrn:partition:service::account_id:resource_type/resource_id
    assert!(result.hrn.starts_with("hrn:"), "HRN should start with 'hrn:'");
    assert!(result.hrn.contains(":iam:"), "HRN should contain service 'iam' in lowercase");
    assert!(result.hrn.contains(":User/"), "HRN should contain resource_type 'User' followed by '/'");
}

#[tokio::test]
async fn test_create_user_unique_ids() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let command = CreateUserCommand {
        name: "Same Name".to_string(),
        email: "same@example.com".to_string(),
        tags: vec![],
    };

    let result1 = use_case.execute(command.clone()).await.unwrap();
    let result2 = use_case.execute(command.clone()).await.unwrap();

    // Even with same data, HRNs should be different (UUID)
    assert_ne!(result1.hrn, result2.hrn);
}

#[tokio::test]
async fn test_create_users_batch() {
    let repo = Arc::new(InMemoryUserRepository::new());
    let use_case = create_user::di::make_use_case(repo.clone());

    let users = vec![
        ("Alice", "alice@test.com"),
        ("Bob", "bob@test.com"),
        ("Charlie", "charlie@test.com"),
    ];

    for (name, email) in users {
        let command = CreateUserCommand {
            name: name.to_string(),
            email: email.to_string(),
            tags: vec![],
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }

    let all_users = repo.find_all().await.unwrap();
    assert_eq!(all_users.len(), 3);
}
</file>

<file path="crates/hodei-iam/tests/unit_group_test.rs">
/// Unit tests for Group domain entity
use hodei_iam::shared::domain::Group;
use policies::shared::domain::hrn::Hrn;

#[test]
fn test_group_new_creates_empty_collections() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let group = Group::new(hrn, "Developers".to_string());

    assert_eq!(group.name, "Developers");
    assert_eq!(group.tags.len(), 0);
    assert_eq!(group.attached_policies().len(), 0);
}

#[test]
fn test_group_attach_policy_idempotent() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let mut group = Group::new(hrn, "Developers".to_string());

    let policy_hrn = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "policy1".into(),
    );

    // Attach policy twice
    group.attach_policy(policy_hrn.clone());
    group.attach_policy(policy_hrn.clone());

    // Should only have one policy
    assert_eq!(group.attached_policies().len(), 1);
    assert_eq!(group.attached_policies()[0], policy_hrn);
}

#[test]
fn test_group_detach_policy() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let mut group = Group::new(hrn, "Developers".to_string());

    let policy1 = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "p1".into(),
    );
    let policy2 = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "p2".into(),
    );

    group.attach_policy(policy1.clone());
    group.attach_policy(policy2.clone());
    assert_eq!(group.attached_policies().len(), 2);

    group.detach_policy(&policy1);
    assert_eq!(group.attached_policies().len(), 1);
    assert_eq!(group.attached_policies()[0], policy2);
}

#[test]
fn test_group_detach_nonexistent_policy_does_nothing() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let mut group = Group::new(hrn, "Developers".to_string());

    let policy_hrn = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "p1".into(),
    );

    // Detach policy that doesn't exist
    group.detach_policy(&policy_hrn);

    assert_eq!(group.attached_policies().len(), 0);
}

#[test]
fn test_group_name_getter() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let group = Group::new(hrn, "Developers".to_string());

    assert_eq!(group.group_name(), "Developers");
}

#[test]
fn test_group_multiple_policies() {
    let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
    let mut group = Group::new(hrn, "Developers".to_string());

    let policy1 = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "p1".into(),
    );
    let policy2 = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "p2".into(),
    );
    let policy3 = Hrn::new(
        "hodei".into(),
        "policies".into(),
        "default".into(),
        "Policy".into(),
        "p3".into(),
    );

    group.attach_policy(policy1);
    group.attach_policy(policy2);
    group.attach_policy(policy3);

    assert_eq!(group.attached_policies().len(), 3);
}
</file>

<file path="crates/hodei-iam/tests/unit_user_test.rs">
/// Unit tests for User domain entity
use hodei_iam::shared::domain::User;
use policies::shared::domain::hrn::Hrn;

#[test]
fn test_user_new_creates_empty_groups() {
    let hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let user = User::new(hrn, "Alice".to_string(), "alice@test.com".to_string());

    assert_eq!(user.name, "Alice");
    assert_eq!(user.email, "alice@test.com");
    assert_eq!(user.groups().len(), 0);
    assert_eq!(user.tags.len(), 0);
}

#[test]
fn test_user_add_to_group_idempotent() {
    let user_hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let mut user = User::new(user_hrn, "Alice".to_string(), "alice@test.com".to_string());

    let group_hrn = Hrn::new(
        "hodei".into(),
        "IAM".into(),
        "default".into(),
        "Group".into(),
        "devs".into(),
    );

    // Add group twice
    user.add_to_group(group_hrn.clone());
    user.add_to_group(group_hrn.clone());

    // Should only have one group
    assert_eq!(user.groups().len(), 1);
    assert_eq!(user.groups()[0], group_hrn);
}

#[test]
fn test_user_remove_from_group() {
    let user_hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let mut user = User::new(user_hrn, "Alice".to_string(), "alice@test.com".to_string());

    let group1 = Hrn::new(
        "hodei".into(),
        "IAM".into(),
        "default".into(),
        "Group".into(),
        "devs".into(),
    );
    let group2 = Hrn::new(
        "hodei".into(),
        "IAM".into(),
        "default".into(),
        "Group".into(),
        "ops".into(),
    );

    user.add_to_group(group1.clone());
    user.add_to_group(group2.clone());
    assert_eq!(user.groups().len(), 2);

    user.remove_from_group(&group1);
    assert_eq!(user.groups().len(), 1);
    assert_eq!(user.groups()[0], group2);
}

#[test]
fn test_user_remove_nonexistent_group_does_nothing() {
    let user_hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let mut user = User::new(user_hrn, "Alice".to_string(), "alice@test.com".to_string());

    let group_hrn = Hrn::new(
        "hodei".into(),
        "IAM".into(),
        "default".into(),
        "Group".into(),
        "devs".into(),
    );

    // Remove group that doesn't exist
    user.remove_from_group(&group_hrn);

    assert_eq!(user.groups().len(), 0);
}

#[test]
fn test_user_email_getter() {
    let user_hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
    let user = User::new(
        user_hrn,
        "Alice".to_string(),
        "alice@example.com".to_string(),
    );

    assert_eq!(user.email(), "alice@example.com");
}
</file>

<file path="crates/hodei-iam/Cargo.toml">
[package]
name = "hodei-iam"
version = "0.1.0"
edition = "2024"

[dependencies]
policies = { path = "../policies" }
shared = { path = "../shared" }
cedar-policy = { workspace = true }
serde = { workspace = true }
chrono = { workspace = true }
anyhow = "1.0"
async-trait = "0.1"
tokio = { workspace = true }
uuid = { version = "1.0", features = ["v4", "serde"] }
tracing = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["full", "test-util"] }
</file>

<file path="crates/hodei-organizations/src/features/create_account/adapter.rs">
use crate::features::create_account::error::CreateAccountError;
use crate::features::create_account::ports::AccountPersister;
use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::domain::account::Account;
use async_trait::async_trait;

/// Adapter implementing AccountPersister over any AccountRepository.
/// Creation is done via `AccountRepositoryAdapter::new(repo)`. The previous
/// `account_persister` helper was removed to avoid dead_code warnings and
/// simplify DI wiring.
pub(crate) struct AccountRepositoryAdapter<AR: AccountRepository> {
    repository: AR,
}

impl<AR: AccountRepository> AccountRepositoryAdapter<AR> {
    pub(crate) fn new(repository: AR) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<AR: AccountRepository + Send + Sync> AccountPersister for AccountRepositoryAdapter<AR> {
    async fn save(&self, account: Account) -> Result<(), CreateAccountError> {
        <AR as AccountRepository>::save(&self.repository, &account)
            .await
            .map_err(CreateAccountError::AccountRepositoryError)
    }
}
</file>

<file path="crates/hodei-organizations/src/features/get_effective_scps/use_case.rs">
use crate::features::get_effective_scps::dto::{EffectiveScpsResponse, GetEffectiveScpsQuery};
use crate::features::get_effective_scps::error::GetEffectiveScpsError;
use crate::features::get_effective_scps::ports::{
    AccountRepositoryPort, OuRepositoryPort, ScpRepositoryPort,
};
use crate::shared::domain::scp::ServiceControlPolicy;
use cedar_policy::PolicySet;
use policies::shared::domain::hrn::Hrn;
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
</file>

<file path="crates/hodei-organizations/src/features/move_account/mocks.rs">
use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;
use std::sync::Arc;

use crate::features::move_account::error::MoveAccountError;
use crate::features::move_account::ports::{MoveAccountUnitOfWork, MoveAccountUnitOfWorkFactory};
use crate::shared::application::ports::account_repository::AccountRepository;
use crate::shared::application::ports::ou_repository::OuRepository;
use crate::shared::domain::account::Account;
use crate::shared::domain::ou::OrganizationalUnit;

/// Mock UnitOfWork for testing transactional behavior
pub struct MockMoveAccountUnitOfWork {
    pub should_fail_on_save: bool,
    pub save_calls: Arc<std::sync::Mutex<Vec<String>>>,
    pub transaction_active: bool,
}

impl Default for MockMoveAccountUnitOfWork {
    fn default() -> Self {
        Self::new()
    }
}

impl MockMoveAccountUnitOfWork {
    pub fn new() -> Self {
        Self {
            should_fail_on_save: false,
            save_calls: Arc::new(std::sync::Mutex::new(Vec::new())),
            transaction_active: false,
        }
    }

    pub fn with_failure(should_fail: bool) -> Self {
        Self {
            should_fail_on_save: should_fail,
            save_calls: Arc::new(std::sync::Mutex::new(Vec::new())),
            transaction_active: false,
        }
    }
}

#[async_trait]
impl MoveAccountUnitOfWork for MockMoveAccountUnitOfWork {
    async fn begin(&mut self) -> Result<(), MoveAccountError> {
        self.transaction_active = true;
        Ok(())
    }

    async fn commit(&mut self) -> Result<(), MoveAccountError> {
        if !self.transaction_active {
            return Err(MoveAccountError::OuRepositoryError(
                crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError(
                    "No transaction in progress".to_string(),
                ),
            ));
        }
        self.transaction_active = false;
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), MoveAccountError> {
        if !self.transaction_active {
            return Err(MoveAccountError::OuRepositoryError(
                crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError(
                    "No transaction in progress".to_string(),
                ),
            ));
        }
        self.transaction_active = false;
        Ok(())
    }

    fn accounts(&self) -> Arc<dyn AccountRepository> {
        Arc::new(MockAccountRepository {
            should_fail_on_save: self.should_fail_on_save,
            save_calls: self.save_calls.clone(),
        })
    }

    fn ous(&self) -> Arc<dyn OuRepository> {
        Arc::new(MockOuRepository {
            should_fail_on_save: self.should_fail_on_save,
            save_calls: self.save_calls.clone(),
        })
    }
}

/// Mock AccountRepository for testing
pub struct MockAccountRepository {
    pub should_fail_on_save: bool,
    pub save_calls: Arc<std::sync::Mutex<Vec<String>>>,
}

#[async_trait]
impl AccountRepository for MockAccountRepository {
    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<
        Option<Account>,
        crate::shared::application::ports::account_repository::AccountRepositoryError,
    > {
        // Return a mock account for testing
        // Match by resource_id instead of full string representation
        if hrn.resource_id == "test" && hrn.resource_type == "account" {
            let source_ou_hrn = Hrn::new(
                "aws".to_string(),
                "hodei".to_string(),
                "123456789012".to_string(),
                "ou".to_string(),
                "source".to_string(),
            );
            Ok(Some(Account::new(
                hrn.clone(),
                "Test Account".to_string(),
                Some(source_ou_hrn),
            )))
        } else {
            Ok(None)
        }
    }

    async fn save(
        &self,
        account: &Account,
    ) -> Result<(), crate::shared::application::ports::account_repository::AccountRepositoryError>
    {
        let mut calls = self.save_calls.lock().unwrap();
        calls.push(format!("account:{}", account.hrn));

        if self.should_fail_on_save {
            Err(crate::shared::application::ports::account_repository::AccountRepositoryError::DatabaseError("Mock save failure".to_string()))
        } else {
            Ok(())
        }
    }
}

/// Mock OuRepository for testing
pub struct MockOuRepository {
    pub should_fail_on_save: bool,
    pub save_calls: Arc<std::sync::Mutex<Vec<String>>>,
}

#[async_trait]
impl OuRepository for MockOuRepository {
    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<
        Option<OrganizationalUnit>,
        crate::shared::application::ports::ou_repository::OuRepositoryError,
    > {
        // Return mock OUs for testing
        // Match by resource_id instead of full string representation
        if hrn.resource_type != "ou" {
            return Ok(None);
        }

        match hrn.resource_id.as_str() {
            "source" => {
                let mut child_accounts = std::collections::HashSet::new();
                let account_hrn = Hrn::new(
                    "aws".to_string(),
                    "hodei".to_string(),
                    "123456789012".to_string(),
                    "account".to_string(),
                    "test".to_string(),
                );
                child_accounts.insert(account_hrn);

                let parent_hrn = Hrn::new(
                    "aws".to_string(),
                    "hodei".to_string(),
                    "123456789012".to_string(),
                    "ou".to_string(),
                    "root".to_string(),
                );

                Ok(Some(OrganizationalUnit {
                    hrn: hrn.clone(),
                    parent_hrn,
                    name: "Source OU".to_string(),
                    child_ous: std::collections::HashSet::new(),
                    child_accounts,
                    attached_scps: std::collections::HashSet::new(),
                }))
            }
            "target" => {
                let parent_hrn = Hrn::new(
                    "aws".to_string(),
                    "hodei".to_string(),
                    "123456789012".to_string(),
                    "ou".to_string(),
                    "root".to_string(),
                );

                Ok(Some(OrganizationalUnit {
                    hrn: hrn.clone(),
                    parent_hrn,
                    name: "Target OU".to_string(),
                    child_ous: std::collections::HashSet::new(),
                    child_accounts: std::collections::HashSet::new(),
                    attached_scps: std::collections::HashSet::new(),
                }))
            }
            _ => Ok(None),
        }
    }

    async fn save(
        &self,
        ou: &OrganizationalUnit,
    ) -> Result<(), crate::shared::application::ports::ou_repository::OuRepositoryError> {
        let mut calls = self.save_calls.lock().unwrap();
        calls.push(format!("ou:{}", ou.hrn));

        if self.should_fail_on_save {
            Err(
                crate::shared::application::ports::ou_repository::OuRepositoryError::DatabaseError(
                    "Mock save failure".to_string(),
                ),
            )
        } else {
            Ok(())
        }
    }
}

/// Mock UnitOfWorkFactory for testing
pub struct MockMoveAccountUnitOfWorkFactory {
    pub should_fail_on_save: bool,
}

impl Default for MockMoveAccountUnitOfWorkFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl MockMoveAccountUnitOfWorkFactory {
    pub fn new() -> Self {
        Self {
            should_fail_on_save: false,
        }
    }

    pub fn with_failure(should_fail: bool) -> Self {
        Self {
            should_fail_on_save: should_fail,
        }
    }
}

#[async_trait]
impl MoveAccountUnitOfWorkFactory for MockMoveAccountUnitOfWorkFactory {
    type UnitOfWork = MockMoveAccountUnitOfWork;

    async fn create(&self) -> Result<Self::UnitOfWork, MoveAccountError> {
        Ok(MockMoveAccountUnitOfWork::with_failure(
            self.should_fail_on_save,
        ))
    }
}
</file>

<file path="crates/hodei-organizations/src/shared/domain/mod.rs">
// Módulos de dominio - INTERNOS al crate
pub(crate) mod account;
pub(crate) mod ou;
pub(crate) mod scp;

// Módulo de eventos - PÚBLICO para suscriptores externos
pub mod events;

// HRN helper para uso interno
pub(crate) mod hrn {
    // Import y helper solo disponibles en tests (evita unused_imports y unexpected cfg)
    #[cfg(test)]
    pub(crate) use policies::shared::domain::hrn::Hrn;

    // Compat helper para tests legacy que usaban Hrn::generate("ou")
    #[cfg(test)]
    pub(crate) fn generate(resource_type: &str) -> Hrn {
        Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            resource_type.to_string(),
            format!("{}-gen", resource_type),
        )
    }
}

// ❌ NO exportar entidades públicamente - solo accesibles dentro del crate
// Las entidades se usan internamente en los casos de uso
// Los casos de uso devuelven DTOs, NO entidades
pub(crate) use account::Account;
pub(crate) use ou::OrganizationalUnit;
pub(crate) use scp::ServiceControlPolicy;

// Re-exportar eventos para conveniencia
pub use events::*;

#[cfg(test)]
mod tests {
    use super::hrn;

    #[test]
    fn hrn_generate_helper_produces_expected_suffix() {
        // When
        let generated = hrn::generate("ou");
        let s = generated.to_string();
        // Then
        assert!(
            s.contains("ou-gen"),
            "Expected generated HRN string to contain 'ou-gen', got {s}"
        );
    }
}
</file>

<file path="crates/hodei-organizations/Cargo.toml">
[package]
name = "hodei-organizations"
version = "0.1.0"
edition = "2024"

[dependencies]
shared = { path = "../shared" }
policies = { path = "../policies" }
surrealdb = { workspace = true }
serde = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
tokio = { workspace = true }
cedar-policy = { workspace = true }
tracing = { workspace = true }
chrono = { workspace = true }


[dev-dependencies]
</file>

<file path="crates/policies/src/features/batch_eval/use_case.rs">
use super::dto::{BatchPlaygroundRequest, BatchPlaygroundResponse};
use cedar_policy::{Entities, PolicySet};

use crate::features::policy_playground::dto as base;
use crate::shared::application::parallel::{
    AuthScenario, build_entities as build_entities_shared,
    build_policy_set as build_policy_set_shared, evaluate_scenarios_channel,
};
use tracing::info;

#[derive(Default)]
pub struct BatchEvalUseCase;

impl BatchEvalUseCase {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(
        &self,
        req: &BatchPlaygroundRequest,
    ) -> Result<BatchPlaygroundResponse, String> {
        // Apply limit
        let scenarios = if let Some(limit) = req.limit_scenarios {
            req.scenarios
                .iter()
                .take(limit)
                .cloned()
                .collect::<Vec<_>>()
        } else {
            req.scenarios.clone()
        };

        // Build shared PolicySet and Entities
        let pset = build_policy_set_shared(&req.policies).unwrap_or_else(|_| PolicySet::new());
        let entity_tuples: Vec<(
            String,
            std::collections::HashMap<String, serde_json::Value>,
            Vec<String>,
        )> = req
            .entities
            .iter()
            .map(|e| (e.uid.clone(), e.attributes.clone(), e.parents.clone()))
            .collect();
        let ents = build_entities_shared(&entity_tuples).unwrap_or_else(|_| Entities::empty());

        // Build scenarios for the evaluator
        let total = scenarios.len();
        let auth_scenarios: Vec<AuthScenario> = scenarios
            .into_iter()
            .map(|s| AuthScenario {
                name: s.name,
                principal: s.principal,
                action: s.action,
                resource: s.resource,
                context: s.context,
            })
            .collect();

        // Use mpsc-based evaluator
        let workers = 8usize;
        let buffer = 2 * workers;
        let (outcomes, pstats) = evaluate_scenarios_channel(
            &pset,
            &ents,
            auth_scenarios,
            req.timeout_ms,
            workers,
            buffer,
        )
        .await?;

        let mut total_time = 0u64;
        let mut allow_count = 0usize;
        for o in outcomes.iter() {
            total_time += o.eval_time_us;
            if o.allow {
                allow_count += 1;
            }
        }

        // redundant redefinition removed (total already computed above)
        let statistics = base::EvaluationStatistics {
            total_scenarios: total,
            allow_count,
            deny_count: total.saturating_sub(allow_count),
            total_evaluation_time_us: total_time,
            average_evaluation_time_us: if total == 0 {
                0
            } else {
                total_time / total as u64
            },
        };

        info!(
            scenarios_total = total,
            timeouts = pstats.timeouts,
            total_eval_time_us = pstats.total_eval_time_us,
            "batch_eval completed"
        );

        Ok(BatchPlaygroundResponse {
            results_count: total,
            statistics,
        })
    }
}
</file>

<file path="crates/policies/src/features/delete_policy/dto.rs">
#[derive(Debug, Clone)]
pub struct DeletePolicyCommand {
    pub policy_id: String,
}

impl DeletePolicyCommand {
    pub fn new(policy_id: impl Into<String>) -> Self {
        Self {
            policy_id: policy_id.into(),
        }
    }

    pub fn validate(&self) -> Result<(), DeletePolicyValidationError> {
        if self.policy_id.trim().is_empty() {
            return Err(DeletePolicyValidationError::EmptyPolicyId);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeletePolicyValidationError {
    #[error("policy id cannot be empty")]
    EmptyPolicyId,
}
</file>

<file path="crates/policies/src/features/delete_policy/mod.rs">
pub mod di;
pub mod dto;
pub mod use_case;
</file>

<file path="crates/policies/src/features/get_policy/dto.rs">
#[derive(Debug, Clone)]
pub struct GetPolicyQuery {
    pub policy_id: String,
}

impl GetPolicyQuery {
    pub fn new(policy_id: impl Into<String>) -> Self {
        Self {
            policy_id: policy_id.into(),
        }
    }

    pub fn validate(&self) -> Result<(), GetPolicyValidationError> {
        if self.policy_id.trim().is_empty() {
            return Err(GetPolicyValidationError::EmptyPolicyId);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GetPolicyValidationError {
    #[error("policy id cannot be empty")]
    EmptyPolicyId,
}
</file>

<file path="crates/policies/src/features/get_policy/mod.rs">
pub mod di;
pub mod dto;
pub mod use_case;
</file>

<file path="crates/policies/src/features/list_policies/dto.rs">
#[derive(Debug, Clone)]
pub struct ListPoliciesQuery {
    /// Pagination: number of items to skip
    pub offset: Option<usize>,
    /// Pagination: maximum number of items to return
    pub limit: Option<usize>,
    /// Filter: only return policies with IDs containing this string
    pub filter_id: Option<String>,
}

impl ListPoliciesQuery {
    pub fn new() -> Self {
        Self {
            offset: None,
            limit: None,
            filter_id: None,
        }
    }

    pub fn with_pagination(offset: usize, limit: usize) -> Self {
        Self {
            offset: Some(offset),
            limit: Some(limit),
            filter_id: None,
        }
    }

    pub fn with_filter(filter_id: String) -> Self {
        Self {
            offset: None,
            limit: None,
            filter_id: Some(filter_id),
        }
    }

    pub fn validate(&self) -> Result<(), ListPoliciesValidationError> {
        // Validate limit is reasonable
        if let Some(limit) = self.limit {
            if limit == 0 {
                return Err(ListPoliciesValidationError::InvalidLimit(
                    "Limit must be greater than 0".to_string(),
                ));
            }
            if limit > 1000 {
                return Err(ListPoliciesValidationError::InvalidLimit(
                    "Limit cannot exceed 1000".to_string(),
                ));
            }
        }
        Ok(())
    }
}

impl Default for ListPoliciesQuery {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ListPoliciesValidationError {
    #[error("invalid limit: {0}")]
    InvalidLimit(String),
}
</file>

<file path="crates/policies/src/features/list_policies/mod.rs">
pub mod di;
pub mod dto;
pub mod use_case;
</file>

<file path="crates/policies/src/features/policy_analysis/use_case.rs">
use super::dto::{AnalyzePoliciesRequest, AnalyzePoliciesResponse, RuleViolation};
use crate::shared::application::parallel::{evaluate_until_first, AuthScenario};
use cedar_policy::{
    Entities, EntityUid, Policy, PolicySet, Schema, SchemaFragment, ValidationMode, Validator,
};
use std::str::FromStr;

#[derive(Default)]
pub struct AnalyzePoliciesUseCase;

impl AnalyzePoliciesUseCase {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(
        &self,
        req: &AnalyzePoliciesRequest,
    ) -> Result<AnalyzePoliciesResponse, String> {
        // Build PolicySet once (fail fast on invalid)
        let mut pset = PolicySet::new();
        for (i, p) in req.policies.iter().enumerate() {
            let pol: Policy = p
                .parse()
                .map_err(|e| format!("policy[{}] parse error: {}", i, e))?;
            pset.add(pol)
                .map_err(|e| format!("policy[{}] add error: {}", i, e))?;
        }

        // Heuristic + semantic checks
        let mut violations: Vec<RuleViolation> = Vec::new();

        // Optional: schema-based validation of policy set
        if let Some(s) = &req.schema
            && let Ok((frag, _)) = SchemaFragment::from_cedarschema_str(s)
                && let Ok(schema) = Schema::from_schema_fragments(vec![frag]) {
                    let v = Validator::new(schema);
                    let vr = v.validate(&pset, ValidationMode::default());
                    if !vr.validation_passed() {
                        for e in vr.validation_errors() {
                            violations.push(RuleViolation {
                                rule_id: "validator".to_string(),
                                message: e.to_string(),
                            });
                        }
                    }
                }

        for rule in &req.rules {
            match rule.kind.as_str() {
                "no_permit_without_mfa" => {
                    let principal = synth_euid("User", "synthetic").to_string();
                    let action = synth_euid("Action", "view").to_string();
                    let resource = synth_euid("Resource", "doc1").to_string();
                    let mut ctx_false = std::collections::HashMap::new();
                    ctx_false.insert("mfa".to_string(), serde_json::json!(false));
                    let scenarios = vec![
                        AuthScenario {
                            name: "mfa_false".to_string(),
                            principal: principal.clone(),
                            action: action.clone(),
                            resource: resource.clone(),
                            context: Some(ctx_false),
                        },
                        AuthScenario {
                            name: "mfa_missing".to_string(),
                            principal: principal.clone(),
                            action: action.clone(),
                            resource: resource.clone(),
                            context: None,
                        },
                    ];
                    if let Some(out) = evaluate_until_first(
                        &pset,
                        &Entities::empty(),
                        scenarios,
                        None,
                        4,
                        8,
                        |o| o.allow,
                    )
                    .await?
                    {
                        violations.push(RuleViolation {
                            rule_id: rule.id.clone(),
                            message: format!(
                                "Allow without strong auth: scenario='{}' P='{}' A='{}' R='{}'",
                                out.name, principal, action, resource
                            ),
                        });
                    }
                }
                "no_permit_without_condition" => {
                    let unconditioned = req.policies.iter().any(|p| {
                        let pol = p.to_lowercase();
                        pol.contains("permit(")
                            && !pol.contains(" when ")
                            && !pol.contains("unless ")
                    });
                    if unconditioned {
                        let principal = synth_euid("User", "u").to_string();
                        let action = synth_euid("Action", "a").to_string();
                        let resource = synth_euid("Resource", "r").to_string();
                        let scenarios = vec![AuthScenario {
                            name: "empty_ctx".to_string(),
                            principal: principal.clone(),
                            action: action.clone(),
                            resource: resource.clone(),
                            context: None,
                        }];
                        if let Some(out) = evaluate_until_first(
                            &pset,
                            &Entities::empty(),
                            scenarios,
                            None,
                            2,
                            4,
                            |o| o.allow,
                        )
                        .await?
                        {
                            violations.push(RuleViolation {
                                rule_id: rule.id.clone(),
                                message: format!(
                                    "Allow without condition: scenario='{}' P='{}' A='{}' R='{}'",
                                    out.name, principal, action, resource
                                ),
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(AnalyzePoliciesResponse {
            passed: violations.is_empty(),
            violations,
        })
    }
}

fn synth_euid(etype: &str, name: &str) -> EntityUid {
    // Fall back to common types used in our playground
    let et = match etype {
        "User" | "user" => "User",
        "Action" | "action" => "Action",
        "Resource" | "resource" => "Resource",
        other => other,
    };
    EntityUid::from_str(&format!("{}::\"{}\"", et, name)).expect("valid synthetic euid")
}
</file>

<file path="crates/policies/src/features/policy_playground/use_case.rs">
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::time::Instant;

use crate::shared::application::parallel::{evaluate_scenarios_channel, AuthScenario};
use cedar_policy::{Authorizer, Context, Decision as CedarDecision, Entities, Entity, EntityUid, Policy, PolicySet, Request, RestrictedExpression, Schema, SchemaFragment, ValidationMode, Validator};

use super::dto::{
    AuthorizationDiagnostics, AuthorizationResult, Decision, EntityDefinition, EvaluationStatistics,
    PlaygroundRequest, PlaygroundResponse, PolicyValidationResult, SchemaValidationResult,
    ValidationError, ValidationWarning,
};

#[derive(Debug, thiserror::Error)]
pub enum PlaygroundError {
    #[error("invalid_request: {0}")]
    InvalidRequest(String),
    #[error("policy_parse_error: {0}")]
    PolicyParseError(String),
    #[error("euid_parse_error: {0}")]
    EuidParseError(String),
    #[error("request_build_error: {0}")]
    RequestError(String),
    #[error("schema_parse_error: {0}")]
    SchemaParseError(String),
    #[error("entity_parse_error: {0}")]
    EntityParseError(String),
}

// Helper: map serde_json::Value to RestrictedExpression (basic types)
fn json_to_expr(v: &serde_json::Value) -> Option<RestrictedExpression> {
    match v {
        serde_json::Value::String(s) => Some(RestrictedExpression::new_string(s.clone())),
        serde_json::Value::Bool(b) => Some(RestrictedExpression::new_bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(RestrictedExpression::new_long(i))
            } else {
                n.as_f64().map(|f| RestrictedExpression::new_decimal(f.to_string()))
            }
        }
        serde_json::Value::Array(arr) => {
            let elems: Vec<RestrictedExpression> = arr.iter().filter_map(json_to_expr).collect();
            Some(RestrictedExpression::new_set(elems))
        }
        serde_json::Value::Object(map) => {
            let mut rec: std::collections::BTreeMap<String, RestrictedExpression> = std::collections::BTreeMap::new();
            for (k, val) in map.iter() {
                if let Some(expr) = json_to_expr(val) {
                    rec.insert(k.clone(), expr);
                }
            }
            RestrictedExpression::new_record(rec).ok()
        }
        serde_json::Value::Null => None,
    }
}

impl Default for PolicyPlaygroundUseCase {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PolicyPlaygroundUseCase;

impl PolicyPlaygroundUseCase {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(
        &self,
        req: &PlaygroundRequest,
    ) -> Result<PlaygroundResponse, PlaygroundError> {
        if req.policies.is_empty() {
            return Err(PlaygroundError::InvalidRequest(
                "at least one policy is required".to_string(),
            ));
        }
        if req.authorization_requests.is_empty() {
            return Err(PlaygroundError::InvalidRequest(
                "at least one authorization scenario is required".to_string(),
            ));
        }

        // Schema handling
        let schema_validation = self.validate_schema(&req.schema)?;

        // Parse and validate policies
        let (pset, policy_validation) = self.parse_and_validate_policies(&req.policies, &req.schema)?;

        // Build entities
        let entities = self.parse_entities(&req.entities)?;

        // Evaluate scenarios (parallel when >1)
        let mut results = Vec::with_capacity(req.authorization_requests.len());
        let mut total_time = 0u64;
        let mut allow_count = 0usize;

        if req.authorization_requests.len() == 1 {
            // Keep fast single-path
            let authorizer = Authorizer::new();
            let sc = &req.authorization_requests[0];
            let start = Instant::now();
            let principal = EntityUid::from_str(&sc.principal)
                .map_err(|e| PlaygroundError::EuidParseError(format!("principal: {}", e)))?;
            let action = EntityUid::from_str(&sc.action)
                .map_err(|e| PlaygroundError::EuidParseError(format!("action: {}", e)))?;
            let resource = EntityUid::from_str(&sc.resource)
                .map_err(|e| PlaygroundError::EuidParseError(format!("resource: {}", e)))?;
            let context = self.build_context(sc.context.as_ref());
            let request = Request::new(principal, action, resource, context, None)
                .map_err(|e| PlaygroundError::RequestError(e.to_string()))?;
            let resp = authorizer.is_authorized(&request, &pset, &entities);
            let decision = if resp.decision() == CedarDecision::Allow { allow_count += 1; Decision::Allow } else { Decision::Deny };
            let reasons: Vec<String> = resp.diagnostics().reason().map(|r| r.to_string()).collect();
            let eval_time = start.elapsed().as_micros() as u64;
            total_time += eval_time;
            results.push(AuthorizationResult { scenario_name: sc.name.clone(), decision, determining_policies: vec![], evaluated_policies: vec![], diagnostics: AuthorizationDiagnostics { reasons, errors: vec![], info: vec![] }, evaluation_time_us: eval_time });
        } else {
            // Use shared parallel evaluator
            let auth_scenarios: Vec<AuthScenario> = req.authorization_requests
                .iter()
                .cloned()
                .map(|s| AuthScenario { name: s.name, principal: s.principal, action: s.action, resource: s.resource, context: s.context })
                .collect();
            let workers = 8usize;
            let buffer = 2 * workers;
            let (outcomes, _stats) = evaluate_scenarios_channel(&pset, &entities, auth_scenarios, None, workers, buffer)
                .await
                .map_err(PlaygroundError::RequestError)?;
            for o in outcomes {
                if o.allow { allow_count += 1; }
                total_time += o.eval_time_us;
                results.push(AuthorizationResult {
                    scenario_name: o.name,
                    decision: if o.allow { Decision::Allow } else { Decision::Deny },
                    determining_policies: vec![],
                    evaluated_policies: vec![],
                    diagnostics: AuthorizationDiagnostics { reasons: o.reasons, errors: vec![], info: vec![] },
                    evaluation_time_us: o.eval_time_us,
                });
            }
        }

        // Stable order for determinism in tests
        results.sort_by(|a, b| a.scenario_name.cmp(&b.scenario_name));

        let statistics = EvaluationStatistics {
            total_scenarios: results.len(),
            allow_count,
            deny_count: results.len().saturating_sub(allow_count),
            total_evaluation_time_us: total_time,
            average_evaluation_time_us: if results.is_empty() { 0 } else { total_time / results.len() as u64 },
        };

        Ok(PlaygroundResponse {
            policy_validation,
            schema_validation,
            authorization_results: results,
            statistics,
        })
    }

    fn validate_schema(&self, schema_str: &Option<String>) -> Result<SchemaValidationResult, PlaygroundError> {
        if let Some(s) = schema_str {
            let (frag, _warnings) = SchemaFragment::from_cedarschema_str(s)
                .map_err(|e| PlaygroundError::SchemaParseError(format!("{}", e)))?;
            let _schema = Schema::from_schema_fragments(vec![frag])
                .map_err(|e| PlaygroundError::SchemaParseError(format!("{}", e)))?;
            Ok(SchemaValidationResult { is_valid: true, errors: vec![], entity_types_count: 0, actions_count: 0 })
        } else {
            Ok(SchemaValidationResult { is_valid: true, errors: vec![], entity_types_count: 0, actions_count: 0 })
        }
    }

    fn parse_and_validate_policies(
        &self,
        policies: &[String],
        schema: &Option<String>,
    ) -> Result<(PolicySet, PolicyValidationResult), PlaygroundError> {
        let mut pset = PolicySet::new();
        let mut errors = Vec::new();
        let warnings = Vec::<ValidationWarning>::new();

        for (idx, pstr) in policies.iter().enumerate() {
            match pstr.parse::<Policy>() {
                Ok(pol) => {
                    if let Err(e) = pset.add(pol) {
                        errors.push(ValidationError { message: format!("add error: {}", e), policy_id: Some(format!("policy_{}", idx)), line: None, column: None });
                    }
                }
                Err(e) => errors.push(ValidationError { message: format!("parse error: {}", e), policy_id: Some(format!("policy_{}", idx)), line: None, column: None }),
            }
        }

        if errors.is_empty()
            && let Some(s) = schema
            && let Ok((frag, _)) = SchemaFragment::from_cedarschema_str(s)
            && let Ok(schema_obj) = Schema::from_schema_fragments(vec![frag])
        {
            let validator = Validator::new(schema_obj);
            let vr = validator.validate(&pset, ValidationMode::default());
            if !vr.validation_passed() {
                for e in vr.validation_errors() {
                    errors.push(ValidationError { message: e.to_string(), policy_id: None, line: None, column: None });
                }
            }
        }

        Ok((
            pset,
            PolicyValidationResult { is_valid: errors.is_empty(), errors, warnings, policies_count: policies.len() },
        ))
    }

    fn parse_entities(&self, defs: &[EntityDefinition]) -> Result<Entities, PlaygroundError> {
        if defs.is_empty() { return Ok(Entities::empty()); }
        let mut out = Vec::with_capacity(defs.len());
        for d in defs {
            let uid = EntityUid::from_str(&d.uid)
                .map_err(|e| PlaygroundError::EntityParseError(format!("{}", e)))?;
            let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();
            for (k, v) in &d.attributes {
                if let Some(expr) = json_to_expr(v) { attrs.insert(k.clone(), expr); }
            }
            let mut parents: HashSet<EntityUid> = HashSet::new();
            for p in &d.parents {
                parents.insert(EntityUid::from_str(p).map_err(|e| PlaygroundError::EntityParseError(format!("parent: {}", e)))?);
            }
            let ent = Entity::new(uid, attrs, parents).map_err(|e| PlaygroundError::EntityParseError(e.to_string()))?;
            out.push(ent);
        }
        Entities::from_entities(out, None).map_err(|e| PlaygroundError::EntityParseError(e.to_string()))
    }

    fn build_context(&self, ctx: Option<&std::collections::HashMap<String, serde_json::Value>>) -> Context {
        let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
        if let Some(c) = ctx {
            for (k, v) in c {
                if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); }
            }
        }
        Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
    }
}
</file>

<file path="crates/policies/src/features/update_policy/dto.rs">
#[derive(Debug, Clone)]
pub struct UpdatePolicyCommand {
    pub policy_id: String,
    pub new_policy_content: String,
}

impl UpdatePolicyCommand {
    pub fn new(policy_id: String, new_policy_content: String) -> Self {
        Self {
            policy_id,
            new_policy_content,
        }
    }

    pub fn validate(&self) -> Result<(), UpdatePolicyValidationError> {
        // Validar ID no vacío
        if self.policy_id.trim().is_empty() {
            return Err(UpdatePolicyValidationError::EmptyPolicyId);
        }

        // Validar contenido no vacío
        if self.new_policy_content.trim().is_empty() {
            return Err(UpdatePolicyValidationError::EmptyPolicyContent);
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UpdatePolicyValidationError {
    #[error("policy id cannot be empty")]
    EmptyPolicyId,
    #[error("policy content cannot be empty")]
    EmptyPolicyContent,
}
</file>

<file path="crates/policies/src/features/update_policy/mod.rs">
pub mod dto;
pub mod use_case;
pub mod di;
</file>

<file path="crates/policies/src/shared/domain/ports.rs">
use crate::shared::Hrn;
use async_trait::async_trait;
use cedar_policy::{EntityId, EntityTypeName, EntityUid, Policy, RestrictedExpression};
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Underlying storage error: {0}")]
    ProviderError(#[from] Box<dyn std::error::Error + Send + Sync>),
    #[error("Policy parsing error: {0}")]
    ParsingError(String),
}

/// Attribute types to describe Cedar schema attributes in a typed way
#[derive(Debug, Clone)]
pub enum AttributeType {
    Primitive(&'static str), // e.g. "String", "Long", "Bool"
    Set(Box<AttributeType>), // e.g. Set<String>
    EntityId(&'static str),  // e.g. EntityId<Principal> (pass the entity type name)
}

impl AttributeType {
    pub fn to_cedar_decl(&self) -> String {
        match self {
            AttributeType::Primitive(name) => name.to_string(),
            AttributeType::Set(inner) => format!("Set<{}>", inner.to_cedar_decl()),
            AttributeType::EntityId(entity_ty) => format!("EntityId<{}>", entity_ty),
        }
    }
}

/// Type-level metadata for building Cedar schema fragments from Rust types
pub trait HodeiEntityType {
    /// Devuelve el nombre del 'servicio' que actúa como espacio de nombres.
    /// Ejemplo: "IAM", "Billing", "S3".
    fn service_name() -> &'static str;

    /// Devuelve el nombre local del tipo de recurso.
    /// Ejemplo: "User", "Group", "Bucket".
    fn resource_type_name() -> &'static str;

    /// **Método de conveniencia (con implementación por defecto).**
    /// Construye el `EntityTypeName` completo para Cedar a partir de las partes.
    fn cedar_entity_type_name() -> EntityTypeName {
        let namespace = Hrn::to_pascal_case(Self::service_name());
        let type_str = format!("{}::{}", namespace, Self::resource_type_name());
        EntityTypeName::from_str(&type_str)
            .expect("Failed to create EntityTypeName from service and resource type")
    }

    /// DEPRECATED: Use `cedar_entity_type_name()` instead.
    /// Mantener por compatibilidad temporal.
    fn entity_type_name() -> &'static str {
        // Fallback para compatibilidad: usa resource_type_name
        Self::resource_type_name()
    }

    /// Whether this entity type is a Principal in Cedar terms
    fn is_principal_type() -> bool {
        false
    }

    /// Optional: declare attributes in a typed fashion
    /// Default: empty, but recommended to provide for typed schema generation
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        Vec::new()
    }

    /// Optional: declare conceptual parent types (for membership semantics)
    /// Default: empty; membership will be modeled at data level via parents()
    fn cedar_parents_types() -> Vec<&'static str> {
        Vec::new()
    }
}

pub trait HodeiEntity {
    fn hrn(&self) -> &Hrn;
    fn attributes(&self) -> std::collections::HashMap<String, RestrictedExpression>;
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
    fn euid(&self) -> EntityUid {
        let hrn = self.hrn();
        let eid = EntityId::from_str(hrn.resource_id.as_str()).unwrap();
        let type_name: EntityTypeName =
            EntityTypeName::from_str(hrn.resource_type.as_str()).unwrap();
        EntityUid::from_type_name_and_id(type_name, eid)
    }
}

///A marker trait for entities that can act as 'principals'.
pub trait Principal: HodeiEntity + HodeiEntityType {}

/// A marker trait for entities that can act as 'resources'.
pub trait Resource: HodeiEntity + HodeiEntityType {}

/// Define an action that can be registered in thepolicy engine.
pub trait Action {
    /// The unique identifier of the action.
    fn name() -> &'static str;

    /// Define which types of Principal and Resource this action applies to.
    /// This will be used to generate the Cedar schema.
    fn applies_to() -> (EntityTypeName, EntityTypeName);
}

#[async_trait]
pub trait PolicyStorage: Send + Sync {
    async fn save_policy(&self, policy: &Policy) -> Result<(), StorageError>;
    async fn delete_policy(&self, id: &str) -> Result<bool, StorageError>;
    async fn get_policy_by_id(&self, id: &str) -> Result<Option<Policy>, StorageError>;
    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError>;
}
</file>

<file path="crates/policies/src/shared/infrastructure/surreal/embedded_storage.rs">
// Feature gate is already applied at the module level in mod.rs

use crate::shared::domain::ports::{PolicyStorage, StorageError};
use async_trait::async_trait;
use cedar_policy::Policy;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

#[derive(Clone)]
pub struct SurrealEmbeddedStorage {
    db: Surreal<Db>,
    table: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PolicyRecord {
    src: String,
}

impl SurrealEmbeddedStorage {
    /// path: filesystem path for RocksDB directory
    pub async fn new(namespace: &str, database: &str, path: &str) -> Result<Self, StorageError> {
        let db = Surreal::new::<RocksDb>(path)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        db.use_ns(namespace)
            .use_db(database)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(Self {
            db,
            table: "policies".into(),
        })
    }
}

#[async_trait]
impl PolicyStorage for SurrealEmbeddedStorage {
    async fn save_policy(&self, policy: &Policy) -> Result<(), StorageError> {
        let thing = (self.table.as_str(), policy.id().to_string());
        let _res: Option<PolicyRecord> = self
            .db
            .upsert(thing)
            .content(PolicyRecord {
                src: policy.to_string(),
            })
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(())
    }

    async fn delete_policy(&self, id: &str) -> Result<bool, StorageError> {
        let res: Option<PolicyRecord> = self
            .db
            .delete((self.table.as_str(), id))
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(res.is_some())
    }

    async fn get_policy_by_id(&self, id: &str) -> Result<Option<Policy>, StorageError> {
        let thing = (self.table.as_str(), id);
        let rec: Option<PolicyRecord> = self
            .db
            .select(thing)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;

        match rec {
            Some(r) => {
                let policy = r
                    .src
                    .parse::<Policy>()
                    .map_err(|e| StorageError::ParsingError(e.to_string()))?;
                Ok(Some(policy))
            }
            None => Ok(None),
        }
    }

    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError> {
        let recs: Vec<PolicyRecord> = self
            .db
            .select(self.table.as_str())
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        let mut out = Vec::with_capacity(recs.len());
        for r in recs {
            let p = r
                .src
                .parse::<Policy>()
                .map_err(|e| StorageError::ParsingError(e.to_string()))?;
            out.push(p);
        }
        Ok(out)
    }
}
</file>

<file path="crates/policies/src/shared/infrastructure/surreal/mem_storage.rs">
use crate::shared::domain::ports::{PolicyStorage, StorageError};
use async_trait::async_trait;
use cedar_policy::Policy;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;

#[derive(Clone)]
pub struct SurrealMemStorage {
    db: Surreal<Db>,
    _namespace: String,
    _database: String,
    table: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PolicyRecord {
    src: String,
}

impl SurrealMemStorage {
    pub async fn new(namespace: &str, database: &str) -> Result<Self, StorageError> {
        let db = Surreal::new::<Mem>(())
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        db.use_ns(namespace)
            .use_db(database)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(Self {
            db,
            _namespace: namespace.to_string(),
            _database: database.to_string(),
            table: "policies".to_string(),
        })
    }
}

#[async_trait]
impl PolicyStorage for SurrealMemStorage {
    async fn save_policy(&self, policy: &Policy) -> Result<(), StorageError> {
        let thing = (self.table.as_str(), policy.id().to_string());
        let _res: Option<PolicyRecord> = self
            .db
            .upsert(thing)
            .content(PolicyRecord {
                src: policy.to_string(),
            })
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(())
    }

    async fn delete_policy(&self, id: &str) -> Result<bool, StorageError> {
        let thing = (self.table.as_str(), id);
        let res: Option<PolicyRecord> = self
            .db
            .delete(thing)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        Ok(res.is_some())
    }

    async fn get_policy_by_id(&self, id: &str) -> Result<Option<Policy>, StorageError> {
        let thing = (self.table.as_str(), id);
        let rec: Option<PolicyRecord> = self
            .db
            .select(thing)
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;

        match rec {
            Some(r) => {
                let policy = r
                    .src
                    .parse::<Policy>()
                    .map_err(|e| StorageError::ParsingError(e.to_string()))?;
                Ok(Some(policy))
            }
            None => Ok(None),
        }
    }

    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError> {
        let recs: Vec<PolicyRecord> = self
            .db
            .select(self.table.as_str())
            .await
            .map_err(|e| StorageError::ProviderError(Box::new(e)))?;
        let mut out = Vec::with_capacity(recs.len());
        for r in recs {
            let p = r
                .src
                .parse::<Policy>()
                .map_err(|e| StorageError::ParsingError(e.to_string()))?;
            out.push(p);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn can_save_and_load_policy() {
        let storage = SurrealMemStorage::new("test_ns", "test_db")
            .await
            .expect("connect mem surreal");
        let src = r#"permit(principal, action, resource);"#;
        let p: Policy = src.parse().expect("parse policy");
        storage.save_policy(&p).await.expect("save");
        let all = storage.load_all_policies().await.expect("load");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].to_string(), p.to_string());
        let removed = storage
            .delete_policy(&p.id().to_string())
            .await
            .expect("delete");
        assert!(removed);
    }
}
</file>

<file path="crates/policies/tests/test_schema.rs">
//! Test to verify schema implementation works correctly

use cedar_policy::{
    Context, Entities, Entity, EntityUid, PolicySet, Request, RestrictedExpression, Schema,
    SchemaFragment,
};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

// Define a simple test entity that implements HodeiEntity
#[derive(Debug, Clone)]
struct TestUser {
    id: String,
    name: String,
    email: String,
}

impl TestUser {
    fn new(id: String, name: String, email: String) -> Self {
        Self { id, name, email }
    }

    fn euid(&self) -> EntityUid {
        EntityUid::from_str(&format!("User::\"{}\"", self.id)).unwrap()
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );
        attrs.insert(
            "email".to_string(),
            RestrictedExpression::new_string(self.email.clone()),
        );
        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

#[test]
fn test_complete_schema_build() {
    // Test the complete schema
    let schema_str = r#"
    entity Principal { };
    
    entity User in Principal {
        name: String,
        email: String
    };
    
    action access appliesTo {
        principal: User,
        resource: User
    };
    "#;

    let result = SchemaFragment::from_cedarschema_str(schema_str);
    assert!(
        result.is_ok(),
        "Failed to create schema fragment: {:?}",
        result.err()
    );

    let (fragment, warnings) = result.unwrap();
    for warning in warnings {
        println!("Warning: {}", warning);
    }

    // Try to build a complete schema
    let schema_result = Schema::from_schema_fragments([fragment]);
    assert!(
        schema_result.is_ok(),
        "Failed to build complete schema: {:?}",
        schema_result.err()
    );

    let schema = schema_result.unwrap();

    // Try to create a validator
    let _validator = cedar_policy::Validator::new(schema.clone());

    // Test creating an entity with RestrictedExpression attributes
    let user = TestUser::new(
        "test_user".to_string(),
        "Test User".to_string(),
        "test@example.com".to_string(),
    );
    let parents: HashSet<_> = user.parents().into_iter().collect();
    let entity = Entity::new(user.euid(), user.attributes(), parents);
    assert!(
        entity.is_ok(),
        "Failed to create entity: {:?}",
        entity.err()
    );
}

#[test]
fn test_policy_evaluation_with_restricted_expressions() -> Result<(), Box<dyn std::error::Error>> {
    let schema_str = r#"
    entity Principal { };
    
    entity User in Principal {
        name: String,
        email: String
    };
    
    action access appliesTo {
        principal: User,
        resource: User
    };
    "#;

    let (schema_fragment, _) = SchemaFragment::from_cedarschema_str(schema_str)?;
    let schema = Schema::from_schema_fragments([schema_fragment])?;

    // Create a simple policy
    let policy_str = r#"permit(
        principal == User::"alice",
        action == Action::"access",
        resource == User::"bob"
    );"#;

    let policy = policy_str.parse()?;

    // Create entities
    let alice_attrs: HashMap<String, RestrictedExpression> = [
        (
            "name".to_string(),
            RestrictedExpression::new_string("Alice".to_string()),
        ),
        (
            "email".to_string(),
            RestrictedExpression::new_string("alice@example.com".to_string()),
        ),
    ]
    .into_iter()
    .collect();

    let alice_entity = Entity::new(
        EntityUid::from_str(r#"User::"alice""#)?,
        alice_attrs,
        HashSet::new(),
    )?;

    let bob_attrs: HashMap<String, RestrictedExpression> = [
        (
            "name".to_string(),
            RestrictedExpression::new_string("Bob".to_string()),
        ),
        (
            "email".to_string(),
            RestrictedExpression::new_string("bob@example.com".to_string()),
        ),
    ]
    .into_iter()
    .collect();

    let bob_entity = Entity::new(
        EntityUid::from_str(r#"User::"bob""#)?,
        bob_attrs,
        HashSet::new(),
    )?;

    let entities = Entities::from_entities(vec![alice_entity, bob_entity], None).expect("entities");
    let policies = PolicySet::from_policies([policy])?;

    let request = Request::new(
        EntityUid::from_str(r#"User::"alice""#)?,
        EntityUid::from_str(r#"Action::"access""#)?,
        EntityUid::from_str(r#"User::"bob""#)?,
        Context::empty(),
        Some(&schema),
    )?;

    let authorizer = cedar_policy::Authorizer::new();
    let response = authorizer.is_authorized(&request, &policies, &entities);

    assert_eq!(response.decision(), cedar_policy::Decision::Allow);
    Ok(())
}
</file>

<file path="crates/shared/src/enums.rs">
// crates/shared/src/enums.rs

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Ecosistemas de paquetes soportados por el sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ecosystem {
    Maven,
    Npm,
    Docker,
    Oci,
    Pypi,
    Nuget,
    Go,
    RubyGems,
    Helm,
    Generic,
}

impl FromStr for Ecosystem {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Maven" => Ok(Ecosystem::Maven),
            "Npm" => Ok(Ecosystem::Npm),
            "Docker" => Ok(Ecosystem::Docker),
            "Oci" => Ok(Ecosystem::Oci),
            "Pypi" => Ok(Ecosystem::Pypi),
            "Nuget" => Ok(Ecosystem::Nuget),
            "Go" => Ok(Ecosystem::Go),
            "RubyGems" => Ok(Ecosystem::RubyGems),
            "Helm" => Ok(Ecosystem::Helm),
            "Generic" => Ok(Ecosystem::Generic),
            _ => Err(()),
        }
    }
}

/// Algoritmos de hash soportados para la verificación de integridad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HashAlgorithm {
    Sha256,
    Sha512,
    Md5,
}

/// Niveles de severidad de vulnerabilidades, basados en estándares como CVSS.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Ord, PartialOrd, PartialEq, Eq)]
pub enum VulnerabilitySeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
    Unknown,
}

/// El tipo de un fichero físico dentro de un paquete.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactType {
    Primary,
    Signature,
    Sbom,
    Metadata,
    Documentation,
    Sources,
    Other,
}

/// El rol semántico que un fichero físico juega dentro de un `PackageVersion`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactRole {
    Main,
    Pom,
    PackageJson,
    Sources,
    Javadoc,
    TypeDefinitions,
    Signature,
    Sbom,
    Other,
}
</file>

<file path="crates/hodei-authorizer/src/features/evaluate_permissions/di.rs">
use std::sync::Arc;

use crate::features::evaluate_permissions::dto::AuthorizationResponse;
use crate::features::evaluate_permissions::error::EvaluatePermissionsResult;
use crate::features::evaluate_permissions::ports::{
    AuthorizationCache, AuthorizationLogger, AuthorizationMetrics,
};
use crate::features::evaluate_permissions::use_case::EvaluatePermissionsUseCase;
use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;

// ✅ Importar casos de uso de otros crates (NO entidades ni providers)
use hodei_iam::DynEffectivePoliciesQueryService;
use policies::shared::AuthorizationEngine;

// Usar el trait local en lugar del tipo concreto
use crate::features::evaluate_permissions::use_case::GetEffectiveScpsPort;

/// Dummy cache implementation for when cache is not needed
#[derive(Debug, Clone, Copy)]
pub struct DummyCache;

#[async_trait]
impl AuthorizationCache for DummyCache {
    async fn get(
        &self,
        _cache_key: &str,
    ) -> EvaluatePermissionsResult<Option<AuthorizationResponse>> {
        Ok(None)
    }

    async fn put(
        &self,
        _cache_key: &str,
        _response: &AuthorizationResponse,
        _ttl: std::time::Duration,
    ) -> EvaluatePermissionsResult<()> {
        Ok(())
    }

    async fn invalidate_principal(&self, _principal_hrn: &Hrn) -> EvaluatePermissionsResult<()> {
        Ok(())
    }

    async fn invalidate_resource(&self, _resource_hrn: &Hrn) -> EvaluatePermissionsResult<()> {
        Ok(())
    }
}

/// Dependency injection container for the evaluate permissions feature
///
/// Este container inyecta CASOS DE USO de otros crates, NO providers custom.
/// Esto respeta el principio de responsabilidad única.
pub struct EvaluatePermissionsContainer<CACHE, LOGGER, METRICS> {
    // ✅ Casos de uso de otros crates
    iam_use_case: DynEffectivePoliciesQueryService,
    org_use_case: Option<Arc<dyn GetEffectiveScpsPort>>,
    authorization_engine: Arc<AuthorizationEngine>,

    // Aspectos transversales
    cache: Option<CACHE>,
    logger: LOGGER,
    metrics: METRICS,
}

impl<CACHE, LOGGER, METRICS> EvaluatePermissionsContainer<CACHE, LOGGER, METRICS>
where
    CACHE: AuthorizationCache,
    LOGGER: AuthorizationLogger,
    METRICS: AuthorizationMetrics,
{
    /// Create a new dependency injection container
    pub fn new(
        iam_use_case: DynEffectivePoliciesQueryService,
        org_use_case: Option<Arc<dyn GetEffectiveScpsPort>>,
        authorization_engine: Arc<AuthorizationEngine>,
        cache: Option<CACHE>,
        logger: LOGGER,
        metrics: METRICS,
    ) -> Self {
        Self {
            iam_use_case,
            org_use_case,
            authorization_engine,
            cache,
            logger,
            metrics,
        }
    }

    /// Build the EvaluatePermissionsUseCase with all dependencies injected
    pub fn build_use_case(self) -> EvaluatePermissionsUseCase<CACHE, LOGGER, METRICS> {
        EvaluatePermissionsUseCase::new(
            self.iam_use_case,
            self.org_use_case,
            self.authorization_engine,
            self.cache,
            self.logger,
            self.metrics,
        )
    }
}

/// Builder pattern for creating the dependency injection container
pub struct EvaluatePermissionsContainerBuilder<CACHE, LOGGER, METRICS> {
    iam_use_case: Option<DynEffectivePoliciesQueryService>,
    org_use_case: Option<Arc<dyn GetEffectiveScpsPort>>,
    authorization_engine: Option<Arc<AuthorizationEngine>>,
    cache: Option<CACHE>,
    logger: Option<LOGGER>,
    metrics: Option<METRICS>,
}

impl<CACHE, LOGGER, METRICS> EvaluatePermissionsContainerBuilder<CACHE, LOGGER, METRICS>
where
    CACHE: AuthorizationCache,
    LOGGER: AuthorizationLogger,
    METRICS: AuthorizationMetrics,
{
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            iam_use_case: None,
            org_use_case: None,
            authorization_engine: None,
            cache: None,
            logger: None,
            metrics: None,
        }
    }

    /// Set the IAM use case
    pub fn with_iam_use_case(mut self, iam_use_case: DynEffectivePoliciesQueryService) -> Self {
        self.iam_use_case = Some(iam_use_case);
        self
    }

    /// Set the organization use case
    pub fn with_org_use_case(
        mut self,
        org_use_case: Option<Arc<dyn GetEffectiveScpsPort>>,
    ) -> Self {
        self.org_use_case = org_use_case;
        self
    }

    /// Set the AuthorizationEngine
    pub fn with_authorization_engine(
        mut self,
        authorization_engine: Arc<AuthorizationEngine>,
    ) -> Self {
        self.authorization_engine = Some(authorization_engine);
        self
    }

    /// Set the authorization cache (optional)
    pub fn with_cache(mut self, cache: CACHE) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Set the authorization logger
    pub fn with_logger(mut self, logger: LOGGER) -> Self {
        self.logger = Some(logger);
        self
    }

    /// Set the authorization metrics
    pub fn with_metrics(mut self, metrics: METRICS) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Build the container
    pub fn build(self) -> Result<EvaluatePermissionsContainer<CACHE, LOGGER, METRICS>, String> {
        Ok(EvaluatePermissionsContainer::new(
            self.iam_use_case.ok_or("IAM use case is required")?,
            self.org_use_case,
            self.authorization_engine
                .ok_or("AuthorizationEngine is required")?,
            self.cache,
            self.logger.ok_or("Logger is required")?,
            self.metrics.ok_or("Metrics is required")?,
        ))
    }
}

impl<CACHE, LOGGER, METRICS> Default for EvaluatePermissionsContainerBuilder<CACHE, LOGGER, METRICS>
where
    CACHE: AuthorizationCache,
    LOGGER: AuthorizationLogger,
    METRICS: AuthorizationMetrics,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Factory functions for common dependency configurations
pub mod factories {
    use super::*;

    /// Create a container with all required dependencies (no cache)
    pub fn create_without_cache<LOGGER, METRICS>(
        iam_use_case: DynEffectivePoliciesQueryService,
        org_use_case: Option<Arc<dyn GetEffectiveScpsPort>>,
        authorization_engine: Arc<AuthorizationEngine>,
        logger: LOGGER,
        metrics: METRICS,
    ) -> EvaluatePermissionsContainer<DummyCache, LOGGER, METRICS>
    where
        LOGGER: AuthorizationLogger,
        METRICS: AuthorizationMetrics,
    {
        EvaluatePermissionsContainer::new(
            iam_use_case,
            org_use_case,
            authorization_engine,
            Some(DummyCache),
            logger,
            metrics,
        )
    }

    /// Create a container with cache enabled
    pub fn create_with_cache<CACHE, LOGGER, METRICS>(
        iam_use_case: DynEffectivePoliciesQueryService,
        org_use_case: Option<Arc<dyn GetEffectiveScpsPort>>,
        authorization_engine: Arc<AuthorizationEngine>,
        cache: CACHE,
        logger: LOGGER,
        metrics: METRICS,
    ) -> EvaluatePermissionsContainer<CACHE, LOGGER, METRICS>
    where
        CACHE: AuthorizationCache,
        LOGGER: AuthorizationLogger,
        METRICS: AuthorizationMetrics,
    {
        EvaluatePermissionsContainer::new(
            iam_use_case,
            org_use_case,
            authorization_engine,
            Some(cache),
            logger,
            metrics,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::evaluate_permissions::mocks::{
        MockAuthorizationCache, MockAuthorizationLogger, MockAuthorizationMetrics,
    };

    // Mock implementation of the EffectivePoliciesQueryService trait
    struct MockEffectivePoliciesQueryService;

    #[async_trait::async_trait]
    impl hodei_iam::EffectivePoliciesQueryService for MockEffectivePoliciesQueryService {
        async fn get_effective_policies(
            &self,
            _query: hodei_iam::GetEffectivePoliciesQuery,
        ) -> Result<
            hodei_iam::EffectivePoliciesResponse,
            hodei_iam::features::get_effective_policies_for_principal::GetEffectivePoliciesError,
        > {
            use cedar_policy::PolicySet;
            Ok(hodei_iam::EffectivePoliciesResponse::new(
                PolicySet::new(),
                "mock".to_string(),
            ))
        }
    }

    fn create_test_iam_use_case() -> DynEffectivePoliciesQueryService {
        Arc::new(MockEffectivePoliciesQueryService {})
    }

    fn create_test_org_use_case() -> Option<Arc<dyn GetEffectiveScpsPort>> {
        // Para tests, usamos None o un mock que implemente GetEffectiveScpsPort
        None
    }

    /// Helper para crear AuthorizationEngine SOLO para tests del DI
    ///
    /// ⚠️ IMPORTANTE: En código de producción, el AuthorizationEngine debe
    /// construirse en el APPLICATION LEVEL (main.rs), NO en hodei-authorizer.
    fn create_test_authorization_engine() -> Arc<AuthorizationEngine> {
        use policies::shared::domain::ports::PolicyStorage;

        #[derive(Clone)]
        struct TestOnlyStorage;

        #[async_trait]
        impl PolicyStorage for TestOnlyStorage {
            async fn save_policy(
                &self,
                _: &cedar_policy::Policy,
            ) -> Result<(), policies::shared::domain::ports::StorageError> {
                Ok(())
            }
            async fn delete_policy(
                &self,
                _: &str,
            ) -> Result<bool, policies::shared::domain::ports::StorageError> {
                Ok(false)
            }
            async fn get_policy_by_id(
                &self,
                _: &str,
            ) -> Result<Option<cedar_policy::Policy>, policies::shared::domain::ports::StorageError>
            {
                Ok(None)
            }
            async fn load_all_policies(
                &self,
            ) -> Result<Vec<cedar_policy::Policy>, policies::shared::domain::ports::StorageError>
            {
                Ok(vec![])
            }
        }

        let schema_str = r#"
            entity User;
            entity Resource;
            action "read" appliesTo { principal: User, resource: Resource };
        "#;
        let (fragment, _) = cedar_policy::SchemaFragment::from_cedarschema_str(schema_str)
            .expect("Valid test schema");
        let schema = Arc::new(
            cedar_policy::Schema::from_schema_fragments(vec![fragment]).expect("Valid test schema"),
        );

        let store = policies::shared::PolicyStore::new(schema.clone(), Arc::new(TestOnlyStorage));

        Arc::new(AuthorizationEngine { schema, store })
    }

    #[test]
    fn test_builder_pattern() {
        let iam_use_case = create_test_iam_use_case();
        let org_use_case = create_test_org_use_case();
        let authorization_engine = create_test_authorization_engine();
        let cache = MockAuthorizationCache::new();
        let logger = MockAuthorizationLogger::new();
        let metrics = MockAuthorizationMetrics::new();

        let container = EvaluatePermissionsContainerBuilder::new()
            .with_iam_use_case(iam_use_case)
            .with_org_use_case(org_use_case)
            .with_authorization_engine(authorization_engine)
            .with_cache(cache)
            .with_logger(logger)
            .with_metrics(metrics)
            .build();

        assert!(container.is_ok());
    }

    #[test]
    fn test_builder_missing_required_dependency() {
        let result: Result<
            EvaluatePermissionsContainer<
                MockAuthorizationCache,
                MockAuthorizationLogger,
                MockAuthorizationMetrics,
            >,
            String,
        > = EvaluatePermissionsContainerBuilder::new()
            .with_org_use_case(create_test_org_use_case())
            .with_authorization_engine(create_test_authorization_engine())
            .with_logger(MockAuthorizationLogger::new())
            .with_metrics(MockAuthorizationMetrics::new())
            .build();

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.contains("IAM use case is required"));
        }
    }

    #[test]
    fn test_factory_without_cache() {
        let iam_use_case = create_test_iam_use_case();
        let org_use_case = create_test_org_use_case();
        let authorization_engine = create_test_authorization_engine();
        let logger = MockAuthorizationLogger::new();
        let metrics = MockAuthorizationMetrics::new();

        let container = factories::create_without_cache(
            iam_use_case,
            org_use_case,
            authorization_engine,
            logger,
            metrics,
        );

        let _use_case = container.build_use_case();
        assert!(true); // If we get here, construction succeeded
    }

    #[test]
    fn test_factory_with_cache() {
        let iam_use_case = create_test_iam_use_case();
        let org_use_case = create_test_org_use_case();
        let authorization_engine = create_test_authorization_engine();
        let cache = MockAuthorizationCache::new();
        let logger = MockAuthorizationLogger::new();
        let metrics = MockAuthorizationMetrics::new();

        let container = factories::create_with_cache(
            iam_use_case,
            org_use_case,
            authorization_engine,
            cache,
            logger,
            metrics,
        );

        let _use_case = container.build_use_case();
        assert!(true); // If we get here, construction succeeded
    }
}
</file>

<file path="crates/hodei-iam/tests/unit_hrn_constructor_test.rs">
/// Unit tests for Hrn constructor with HodeiEntityType
use hodei_iam::shared::domain::{Group, User};
use policies::shared::domain::hrn::Hrn;

#[test]
fn test_hrn_for_entity_type_user() {
    let hrn = Hrn::for_entity_type::<User>(
        "hodei".to_string(),
        "default".to_string(),
        "user123".to_string(),
    );

    assert_eq!(hrn.partition, "hodei");
    assert_eq!(hrn.service, "iam"); // service_name is normalized to lowercase
    assert_eq!(hrn.account_id, "default");
    assert_eq!(hrn.resource_type, "User");
    assert_eq!(hrn.resource_id, "user123");
}

#[test]
fn test_hrn_for_entity_type_group() {
    let hrn = Hrn::for_entity_type::<Group>(
        "hodei".to_string(),
        "default".to_string(),
        "group456".to_string(),
    );

    assert_eq!(hrn.partition, "hodei");
    assert_eq!(hrn.service, "iam"); // service_name is normalized to lowercase
    assert_eq!(hrn.account_id, "default");
    assert_eq!(hrn.resource_type, "Group");
    assert_eq!(hrn.resource_id, "group456");
}

#[test]
fn test_hrn_for_entity_type_to_string() {
    let hrn = Hrn::for_entity_type::<User>(
        "hodei".to_string(),
        "account1".to_string(),
        "alice".to_string(),
    );

    let hrn_str = hrn.to_string();
    assert!(hrn_str.contains(":iam:")); // service is lowercase in HRN string
    assert!(hrn_str.contains(":User/")); // resource_type followed by /
    assert!(hrn_str.contains("alice"));
}

#[test]
fn test_hrn_for_entity_type_euid() {
    let hrn = Hrn::for_entity_type::<User>(
        "hodei".to_string(),
        "default".to_string(),
        "bob".to_string(),
    );

    let euid = hrn.euid();
    let euid_str = format!("{}", euid);

    assert!(euid_str.contains("Iam::User")); // Cedar namespace is PascalCase
    assert!(euid_str.contains("bob"));
}
</file>

<file path="crates/policies/src/features/create_policy/dto.rs">
#[derive(Debug, Clone)]
pub struct CreatePolicyCommand {
    pub policy_src: String,
}

impl CreatePolicyCommand {
    pub fn new(policy_src: impl Into<String>) -> Self {
        Self {
            policy_src: policy_src.into(),
        }
    }

    pub fn validate(&self) -> Result<(), CreatePolicyValidationError> {
        if self.policy_src.trim().is_empty() {
            return Err(CreatePolicyValidationError::EmptyPolicySource);
        }
        // Additional syntactic checks can be added here if needed
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CreatePolicyValidationError {
    #[error("policy source cannot be empty")]
    EmptyPolicySource,
}
</file>

<file path="crates/policies/src/features/create_policy/mod.rs">
pub mod di;
pub mod dto;
pub mod use_case;
</file>

<file path="crates/policies/src/features/policy_playground/di.rs">
use super::use_case::PolicyPlaygroundUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build PolicyPlaygroundUseCase (no storage required) and an AuthorizationEngine for consistency
/// NOTE: This creates an engine with NO entities registered.
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_policy_playground_use_case_mem() -> Result<(PolicyPlaygroundUseCase, Arc<AuthorizationEngine>)> {
    let (engine, _store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = PolicyPlaygroundUseCase::new();
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_policy_playground_use_case_embedded(
        path: &str,
    ) -> Result<(PolicyPlaygroundUseCase, Arc<AuthorizationEngine>)> {
        let (engine, _store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = PolicyPlaygroundUseCase::new();
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/features/policy_playground_traces/use_case.rs">
use super::dto::{TracedAuthorizationResult, TracedPlaygroundOptions, TracedPlaygroundResponse};
use crate::features::policy_playground::dto as base;
use cedar_policy::{Authorizer, Context, Decision as CedarDecision, Entities, Entity, EntityUid, Policy, PolicySet, Request, RestrictedExpression};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use tokio::task::JoinSet;

pub struct TracedPlaygroundUseCase;

impl Default for TracedPlaygroundUseCase {
    fn default() -> Self {
        Self::new()
    }
}

impl TracedPlaygroundUseCase {
    pub fn new() -> Self { Self }

    pub async fn execute(
        &self,
        options: &TracedPlaygroundOptions,
        base_req: &base::PlaygroundRequest,
        base_uc: &crate::features::policy_playground::use_case::PolicyPlaygroundUseCase,
    ) -> Result<TracedPlaygroundResponse, String> {
        if !options.include_policy_traces {
            // Fast path: no traces, just call base
            let result = base_uc.execute(base_req).await.map_err(|e| e.to_string())?;
            let wrapped: Vec<TracedAuthorizationResult> = result
                .authorization_results
                .into_iter()
                .map(|base_res| TracedAuthorizationResult { base: base_res, determining_policies: None, evaluated_policies: None })
                .collect();
            return Ok(TracedPlaygroundResponse {
                policy_validation: result.policy_validation,
                schema_validation: result.schema_validation,
                authorization_results: wrapped,
                statistics: result.statistics,
            });
        }

        // Heuristic path: DO NOT call base_uc to avoid ID conflicts; replicate minimal logic
        // Parse all policies together as a single PolicySet to get consistent IDs
        let mut policy_set_str = String::new();
        for pstr in base_req.policies.iter() {
            policy_set_str.push_str(pstr.trim());
            policy_set_str.push_str("\n\n");
        }
        
        // Parse the entire PolicySet at once
        let pset_parsed = PolicySet::from_str(&policy_set_str)
            .map_err(|e| format!("failed to parse policy set: {}", e))?;
        
        // Extract policies with their Cedar-assigned IDs
        let mut policies: Vec<(String, Policy)> = Vec::with_capacity(base_req.policies.len());
        for p in pset_parsed.policies() {
            let id = p.id().to_string();
            policies.push((id, p.clone()));
        }

        // Minimal validation result (no schema/policy validation for traces mode)
        let policy_validation = base::PolicyValidationResult {
            is_valid: true,
            errors: vec![],
            warnings: vec![],
            policies_count: policies.len(),
        };
        let schema_validation = base::SchemaValidationResult {
            is_valid: true,
            errors: vec![],
            entity_types_count: 0,
            actions_count: 0,
        };

        // Build Entities from request
        let entities = build_entities(&base_req.entities)?;

        // Authorizer
        let authorizer = Authorizer::new();

        // For each scenario, compute determining policies by removal (parallel per policy)
        let mut traced_results: Vec<TracedAuthorizationResult> = Vec::with_capacity(base_req.authorization_requests.len());
        let mut total_time: u64 = 0;
        let mut allow_count: usize = 0;
        for sc in &base_req.authorization_requests {

            let principal = EntityUid::from_str(&sc.principal).map_err(|e| format!("principal: {}", e))?;
            let action = EntityUid::from_str(&sc.action).map_err(|e| format!("action: {}", e))?;
            let resource = EntityUid::from_str(&sc.resource).map_err(|e| format!("resource: {}", e))?;
            let context = build_context(sc.context.as_ref());
            let request = Request::new(principal, action, resource, context, None).map_err(|e| e.to_string())?;

            let start = std::time::Instant::now();

            // Build full PolicySet - use the parsed one directly to preserve IDs
            let pset_all = pset_parsed.clone();

            // Baseline
            let baseline = authorizer.is_authorized(&request, &pset_all, &entities);
            let baseline_allow = baseline.decision() == CedarDecision::Allow;
            if baseline_allow { allow_count += 1; }

            // Parallel removal
            let mut set: JoinSet<(String, bool)> = JoinSet::new();
            let policy_strings = base_req.policies.clone(); // Keep original strings
            for (i, (pol_id, _)) in policies.iter().enumerate() {
                let pol_id_cloned = pol_id.clone();
                let policy_strings_clone = policy_strings.clone();
                let entities_clone = entities.clone();
                let sc_principal_c = sc.principal.clone();
                let sc_action_c = sc.action.clone();
                let sc_resource_c = sc.resource.clone();
                let sc_context_c = sc.context.clone();
                set.spawn(async move {
                    // Rebuild PolicySet without policy i
                    let mut pset_str = String::new();
                    for (j, pstr) in policy_strings_clone.iter().enumerate() {
                        if i != j {
                            pset_str.push_str(pstr.trim());
                            pset_str.push_str("\n\n");
                        }
                    }
                    let pset = PolicySet::from_str(&pset_str).unwrap_or_else(|_| PolicySet::new());
                    
                    // Recreate request
                    let principal = EntityUid::from_str(&sc_principal_c).unwrap();
                    let action = EntityUid::from_str(&sc_action_c).unwrap();
                    let resource = EntityUid::from_str(&sc_resource_c).unwrap();
                    let context = build_context(sc_context_c.as_ref());
                    let request = Request::new(principal, action, resource, context, None).unwrap();
                    let a = Authorizer::new();
                    let resp = a.is_authorized(&request, &pset, &entities_clone);
                    let allow = resp.decision() == CedarDecision::Allow;
                    (pol_id_cloned, allow)
                });
            }

            let mut determining: Vec<String> = Vec::new();
            while let Some(joined) = set.join_next().await {
                if let Ok((id, allow)) = joined && allow != baseline_allow { determining.push(id); }
            }

            let eval_time = start.elapsed().as_micros() as u64;
            total_time += eval_time;

            let base_result = base::AuthorizationResult {
                scenario_name: sc.name.clone(),
                decision: if baseline_allow { base::Decision::Allow } else { base::Decision::Deny },
                determining_policies: vec![],
                evaluated_policies: vec![],
                diagnostics: base::AuthorizationDiagnostics { reasons: vec![], errors: vec![], info: vec![] },
                evaluation_time_us: eval_time,
            };

            traced_results.push(TracedAuthorizationResult {
                base: base_result,
                determining_policies: Some(determining),
                evaluated_policies: None,
            });
        }

        let statistics = base::EvaluationStatistics {
            total_scenarios: traced_results.len(),
            allow_count,
            deny_count: traced_results.len().saturating_sub(allow_count),
            total_evaluation_time_us: total_time,
            average_evaluation_time_us: if traced_results.is_empty() { 0 } else { total_time / traced_results.len() as u64 },
        };

        Ok(TracedPlaygroundResponse {
            policy_validation,
            schema_validation,
            authorization_results: traced_results,
            statistics,
        })
    }
}

fn build_entities(defs: &[base::EntityDefinition]) -> Result<Entities, String> {
    if defs.is_empty() { return Ok(Entities::empty()); }
    let mut out = Vec::with_capacity(defs.len());
    for d in defs {
        let uid = EntityUid::from_str(&d.uid).map_err(|e| e.to_string())?;
        let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();
        for (k, v) in &d.attributes { if let Some(expr) = json_to_expr(v) { attrs.insert(k.clone(), expr); } }
        let mut parents: HashSet<EntityUid> = HashSet::new();
        for p in &d.parents { parents.insert(EntityUid::from_str(p).map_err(|e| e.to_string())?); }
        let ent = Entity::new(uid, attrs, parents).map_err(|e| e.to_string())?;
        out.push(ent);
    }
    Entities::from_entities(out, None).map_err(|e| e.to_string())
}

fn build_context(ctx: Option<&HashMap<String, serde_json::Value>>) -> Context {
    let mut map: HashMap<String, RestrictedExpression> = HashMap::new();
    if let Some(c) = ctx {
        for (k, v) in c {
            if let Some(expr) = json_to_expr(v) { map.insert(k.clone(), expr); }
        }
    }
    Context::from_pairs(map).unwrap_or_else(|_| Context::empty())
}

fn json_to_expr(v: &serde_json::Value) -> Option<RestrictedExpression> {
    match v {
        serde_json::Value::String(s) => Some(RestrictedExpression::new_string(s.clone())),
        serde_json::Value::Bool(b) => Some(RestrictedExpression::new_bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(RestrictedExpression::new_long(i))
            } else {
                n.as_f64().map(|f| RestrictedExpression::new_decimal(f.to_string()))
            }
        }
        serde_json::Value::Array(arr) => {
            let elems: Vec<RestrictedExpression> = arr.iter().filter_map(json_to_expr).collect();
            Some(RestrictedExpression::new_set(elems))
        }
        serde_json::Value::Object(map) => {
            let mut rec: BTreeMap<String, RestrictedExpression> = BTreeMap::new();
            for (k, val) in map.iter() { if let Some(expr) = json_to_expr(val) { rec.insert(k.clone(), expr); } }
            RestrictedExpression::new_record(rec).ok()
        }
        serde_json::Value::Null => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn determining_policy_includes_forbid_group() {
        // Policies: forbid admins; permit all (no explicit IDs, will be auto-assigned)
        let req = base::PlaygroundRequest {
            policies: vec![
                "forbid(principal in Group::\"admins\", action, resource);".to_string(),
                "permit(principal, action, resource);".to_string(),
            ],
            schema: None,
            entities: vec![
                base::EntityDefinition { uid: "User::\"alice\"".to_string(), attributes: Default::default(), parents: vec!["Group::\"admins\"".to_string()] },
                base::EntityDefinition { uid: "Group::\"admins\"".to_string(), attributes: Default::default(), parents: vec![] },
            ],
            authorization_requests: vec![ base::AuthorizationScenario {
                name: "alice-deny".to_string(),
                principal: "User::\"alice\"".to_string(),
                action: "Action::\"view\"".to_string(),
                resource: "Resource::\"doc1\"".to_string(),
                context: None,
            }],
            options: None,
        };

        let base_uc = crate::features::policy_playground::use_case::PolicyPlaygroundUseCase::default();
        let traced_uc = TracedPlaygroundUseCase::new();
        let opts = TracedPlaygroundOptions { include_policy_traces: true };
        let res = traced_uc.execute(&opts, &req, &base_uc).await.unwrap();
        let det = &res.authorization_results[0].determining_policies;
        
        assert!(det.as_ref().unwrap().len() >= 1);
        // The forbid policy is determining (removing it changes decision from Deny to Allow)
        // Cedar assigns IDs automatically (policy0, policy1, etc.)
        // The determining policy should be one of them (either policy0 or policy1 depending on parse order)
        let determining_policies = det.as_ref().unwrap();
        assert!(
            determining_policies.contains(&"policy0".to_string()) || 
            determining_policies.contains(&"policy1".to_string()),
            "Expected either policy0 or policy1 in determining policies, but got: {:?}",
            determining_policies
        );
    }
}
</file>

<file path="crates/policies/src/features/validate_policy/di.rs">
use super::use_case::ValidatePolicyUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build ValidatePolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: This creates an engine with NO entities registered.
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_validate_policy_use_case_mem() -> Result<(ValidatePolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = ValidatePolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build ValidatePolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_validate_policy_use_case_embedded(
        path: &str,
    ) -> Result<(ValidatePolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = ValidatePolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/features/validate_policy/use_case.rs">
use std::sync::Arc;

use cedar_policy::Policy;

use super::dto::{ValidatePolicyQuery, ValidationError, ValidationResult};
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum ValidatePolicyError {
    #[error("invalid_query: {0}")]
    InvalidQuery(String),
    #[error("validation_error: {0}")]
    ValidationError(String),
}

pub struct ValidatePolicyUseCase {
    store: Arc<PolicyStore>,
}

impl ValidatePolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        query: &ValidatePolicyQuery,
    ) -> Result<ValidationResult, ValidatePolicyError> {
        // 1. Validar query
        query
            .validate()
            .map_err(|e| ValidatePolicyError::InvalidQuery(e.to_string()))?;

        // 2. Intentar parsear la política
        let policy_result: Result<Policy, _> = query.policy_content.parse();

        match policy_result {
            Ok(policy) => {
                // 3. Validar contra el schema usando el validator del store
                match self.store.validate_policy(&policy) {
                    Ok(()) => Ok(ValidationResult {
                        is_valid: true,
                        errors: vec![],
                        warnings: vec![],
                    }),
                    Err(e) => Ok(ValidationResult {
                        is_valid: false,
                        errors: vec![ValidationError {
                            message: e,
                            line: None,
                            column: None,
                        }],
                        warnings: vec![],
                    }),
                }
            }
            Err(e) => Ok(ValidationResult {
                is_valid: false,
                errors: vec![ValidationError {
                    message: format!("Parse error: {}", e),
                    line: None,
                    column: None,
                }],
                warnings: vec![],
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;

    #[tokio::test]
    async fn validate_policy_accepts_valid_policy() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = ValidatePolicyUseCase::new(store);
        let query = ValidatePolicyQuery::new("permit(principal, action, resource);".to_string());
        let result = uc.execute(&query).await.expect("validate policy");

        assert!(result.is_valid);
        assert_eq!(result.errors.len(), 0);
    }

    #[tokio::test]
    async fn validate_policy_rejects_invalid_syntax() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = ValidatePolicyUseCase::new(store);
        let query =
            ValidatePolicyQuery::new("this is not valid cedar syntax".to_string());
        let result = uc.execute(&query).await.expect("validate policy");

        assert!(!result.is_valid);
        assert!(result.errors.len() > 0);
        assert!(result.errors[0].message.contains("Parse error"));
    }

    #[tokio::test]
    async fn validate_policy_rejects_empty_content() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = ValidatePolicyUseCase::new(store);
        let query = ValidatePolicyQuery::new("".to_string());
        let result = uc.execute(&query).await;

        assert!(result.is_err());
        match result {
            Err(ValidatePolicyError::InvalidQuery(_)) => {}
            _ => panic!("Expected InvalidQuery error"),
        }
    }

    #[tokio::test]
    async fn validate_policy_accepts_complex_valid_policy() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = ValidatePolicyUseCase::new(store);
        let complex_policy = r#"
            permit(
                principal,
                action,
                resource
            ) when {
                principal has email
            };
        "#;
        let query = ValidatePolicyQuery::new(complex_policy.to_string());
        let result = uc.execute(&query).await.expect("validate policy");

        assert!(result.is_valid);
        assert_eq!(result.errors.len(), 0);
    }
}
</file>

<file path="crates/policies/src/shared/application/mod.rs">
// application layer
mod engine;
mod store;
pub mod parallel;
pub mod di_helpers;

pub use engine::{AuthorizationEngine, AuthorizationRequest, EngineBuilder};
pub use store::PolicyStore;
</file>

<file path="crates/policies/src/shared/application/store.rs">
use crate::shared::domain::ports::{PolicyStorage, StorageError};
use cedar_policy::{Policy, PolicySet, Schema, Validator};
use std::sync::Arc;

#[derive(Clone)]
pub struct PolicyStore {
    storage: Arc<dyn PolicyStorage>,
    validator: Validator,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::domain::ports::PolicyStorage;
    use async_trait::async_trait;

    #[derive(Clone)]
    struct DummyStorage;

    #[async_trait]
    impl PolicyStorage for DummyStorage {
        async fn save_policy(&self, _policy: &Policy) -> Result<(), StorageError> {
            Ok(())
        }
        async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
            Ok(true)
        }
        async fn get_policy_by_id(&self, _id: &str) -> Result<Option<Policy>, StorageError> {
            Ok(None)
        }
        async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError> {
            Ok(vec![])
        }
    }

    fn minimal_schema() -> Arc<Schema> {
        let minimal_schema = r#"
        entity Principal { };
        action access appliesTo {
            principal: Principal,
            resource: Principal
        };
        "#;
        let (fragment, _) = cedar_policy::SchemaFragment::from_cedarschema_str(minimal_schema)
            .expect("minimal schema valid");
        Arc::new(Schema::from_schema_fragments(vec![fragment]).expect("schema build"))
    }

    #[tokio::test]
    async fn get_current_policy_set_returns_empty_with_dummy_storage() {
        let storage: Arc<dyn PolicyStorage> = Arc::new(DummyStorage);
        let store = PolicyStore::new(minimal_schema(), storage);
        let pset = store.get_current_policy_set().await.expect("policy set");
        // Rendering should be possible
        let rendered = pset.to_cedar();
        assert!(rendered.is_some());
    }

    #[tokio::test]
    async fn remove_policy_calls_storage() {
        let storage: Arc<dyn PolicyStorage> = Arc::new(DummyStorage);
        let store = PolicyStore::new(minimal_schema(), storage);
        let removed = store.remove_policy("any").await.expect("remove ok");
        assert!(removed);
    }
}

impl PolicyStore {
    pub fn new(schema: Arc<Schema>, storage: Arc<dyn PolicyStorage>) -> Self {
        Self {
            storage,
            validator: Validator::new(schema.as_ref().clone()),
        }
    }

    pub async fn add_policy(&self, policy: Policy) -> Result<(), String> {
        // Build a PolicySet containing the single policy to validate
        let mut pset = PolicySet::new();
        pset.add(policy.clone())
            .map_err(|e| format!("Failed to add policy to set: {}", e))?;

        // Validate the policy set using Cedar's validator
        let validation_result = self
            .validator
            .validate(&pset, cedar_policy::ValidationMode::default());

        if validation_result.validation_passed() {
            self.storage
                .save_policy(&policy)
                .await
                .map_err(|e| e.to_string())
        } else {
            let errors: Vec<String> = validation_result
                .validation_errors()
                .map(|e| e.to_string())
                .collect();
            Err(format!("Policy validation failed: {}", errors.join(", ")))
        }
    }

    pub async fn remove_policy(&self, id: &str) -> Result<bool, String> {
        self.storage
            .delete_policy(id)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_current_policy_set(&self) -> Result<PolicySet, StorageError> {
        let policies = self.storage.load_all_policies().await?;
        let mut policy_set = PolicySet::new();
        for policy in policies {
            policy_set
                .add(policy)
                .map_err(|e| StorageError::ParsingError(e.to_string()))?;
        }
        Ok(policy_set)
    }

    pub async fn get_policy(&self, id: &str) -> Result<Option<Policy>, String> {
        self.storage
            .get_policy_by_id(id)
            .await
            .map_err(|e| e.to_string())
    }

    /// Update an existing policy by removing the old one and adding the new one
    pub async fn update_policy(&self, old_id: &str, new_policy: Policy) -> Result<(), String> {
        // Eliminar política antigua
        let removed = self.remove_policy(old_id).await?;
        if !removed {
            return Err(format!("Policy '{}' not found", old_id));
        }

        // Agregar nueva política (esto valida automáticamente)
        self.add_policy(new_policy).await
    }

    /// Validate a policy without persisting it
    pub fn validate_policy(&self, policy: &Policy) -> Result<(), String> {
        let mut pset = PolicySet::new();
        pset.add(policy.clone())
            .map_err(|e| format!("Failed to add policy: {}", e))?;

        let validation_result = self
            .validator
            .validate(&pset, cedar_policy::ValidationMode::default());

        if validation_result.validation_passed() {
            Ok(())
        } else {
            let errors: Vec<String> = validation_result
                .validation_errors()
                .map(|e| e.to_string())
                .collect();
            Err(format!("Validation failed: {}", errors.join(", ")))
        }
    }
}
</file>

<file path="crates/policies/src/lib.rs">
// crates/policies/src/lib.rs

pub mod shared;
pub use shared as domain;
// backward-compatible alias
pub mod features;
</file>

<file path="crates/policies/tests/domain_compilation_test.rs">
//! Test to verify that the domain modules compile correctly

#[cfg(test)]
mod tests {
    use policies::domain::HodeiEntityType;
    use policies::shared::application::EngineBuilder;

    // Tipos de prueba locales que representan entidades del dominio (ahora en IAM)
    struct TestUserType;
    struct TestGroupType;

    impl HodeiEntityType for TestUserType {
        fn service_name() -> &'static str { "IAM" }
        fn resource_type_name() -> &'static str { "User" }
        fn cedar_attributes() -> Vec<(&'static str, policies::domain::AttributeType)> {
            vec![
                ("name", policies::domain::AttributeType::Primitive("String")),
                ("email", policies::domain::AttributeType::Primitive("String")),
            ]
        }
    }

    impl HodeiEntityType for TestGroupType {
        fn service_name() -> &'static str { "IAM" }
        fn resource_type_name() -> &'static str { "Group" }
        fn cedar_attributes() -> Vec<(&'static str, policies::domain::AttributeType)> {
            vec![ ("name", policies::domain::AttributeType::Primitive("String")) ]
        }
    }

    #[test]
    fn test_user_entity_type() {
        assert_eq!(TestUserType::entity_type_name(), "User");
        // cedar_entity_type_name debe incluir el namespace en PascalCase
        let ty = TestUserType::cedar_entity_type_name();
        assert_eq!(ty.to_string(), "Iam::User");
    }

    #[test]
    fn test_group_entity_type() {
        assert_eq!(TestGroupType::entity_type_name(), "Group");
        let ty = TestGroupType::cedar_entity_type_name();
        assert_eq!(ty.to_string(), "Iam::Group");
    }

    #[test]
    fn test_user_cedar_attributes_present() {
        let attrs = TestUserType::cedar_attributes();
        assert!(
            !attrs.is_empty(),
            "TestUserType should define typed cedar_attributes"
        );
    }

    #[test]
    fn test_group_cedar_attributes_present() {
        let attrs = TestGroupType::cedar_attributes();
        assert!(
            !attrs.is_empty(),
            "TestGroupType should define typed cedar_attributes"
        );
    }

    #[test]
    fn test_engine_builder() {
        let _builder = EngineBuilder::new();
        // Just testing that we can create an engine builder
        assert!(true);
    }
}
</file>

<file path="crates/shared/src/events.rs">
// crates/shared/src/events.rs

use serde::{Serialize, Deserialize};
use crate::hrn::{Hrn};
use async_trait::async_trait;

// Nota: Los tipos concretos de eventos (OrganizationEvent, etc.) se definen en sus
// respectivos crates para mantener la cohesión del Bounded Context.
// Este enum actúa como un contenedor universal para el transporte en Kafka.
// use crate::organization::OrganizationEvent;
// use crate::repository::RepositoryEvent;
// use crate::artifact::ArtifactEvent;
// // ... etc.

/// Enumeración de todos los eventos de dominio que pueden fluir por el bus de eventos.
/// Actúa como un sobre que contiene el evento específico de su dominio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainEvent {
    // Organization(OrganizationEvent),
    // Repository(RepositoryEvent),
    // Artifact(ArtifactEvent),
    // Iam(IamEvent),
    // Security(SecurityEvent),
    // SupplyChain(SupplyChainEvent),
}

/// Evento de dominio básico
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Hrn,
    pub r#type: String,
    pub source: String,
    pub timestamp: String, // ISO-8601
    pub data: serde_json::Map<String, serde_json::Value>,
}

/// Flujo de eventos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventStream {
    pub id: String,
    pub name: String,
    pub organization: Hrn,
    pub filters: Vec<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// Suscripción a un flujo de eventos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubscription {
    pub id: Hrn,
    pub stream: String,
    pub subscriber: String,
    pub active: bool,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// Puerto para publicar eventos de dominio. Implementado en infraestructura.
#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, event: &DomainEvent) -> anyhow::Result<()>;
}
</file>

<file path="crates/policies/src/features/delete_policy/use_case.rs">
use std::sync::Arc;

use super::dto::DeletePolicyCommand;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum DeletePolicyError {
    #[error("invalid_command: {0}")]
    InvalidCommand(String),
    #[error("policy_not_found: {0}")]
    NotFound(String),
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct DeletePolicyUseCase {
    store: Arc<PolicyStore>,
}

impl DeletePolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(&self, cmd: &DeletePolicyCommand) -> Result<bool, DeletePolicyError> {
        // Validate command
        cmd.validate()
            .map_err(|e| DeletePolicyError::InvalidCommand(e.to_string()))?;

        // Delete policy from store
        let deleted = self
            .store
            .remove_policy(&cmd.policy_id)
            .await
            .map_err(DeletePolicyError::Storage)?;

        // Return error if policy was not found
        if !deleted {
            return Err(DeletePolicyError::NotFound(cmd.policy_id.clone()));
        }

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;
    use cedar_policy::Policy;

    #[tokio::test]
    async fn delete_policy_removes_policy_when_exists() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // First, create a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        let policy_id = policy.id().to_string();

        store.add_policy(policy.clone()).await.expect("add policy");

        // Verify it exists
        let retrieved = store.get_policy(&policy_id).await.expect("get policy");
        assert!(retrieved.is_some());

        // Now delete it
        let uc = DeletePolicyUseCase::new(store.clone());
        let cmd = DeletePolicyCommand::new(policy_id.clone());
        let result = uc.execute(&cmd).await.expect("delete policy");

        assert!(result);

        // Verify it's gone
        let retrieved_after = store
            .get_policy(&policy_id)
            .await
            .expect("get policy after delete");
        assert!(retrieved_after.is_none());
    }

    #[tokio::test]
    async fn delete_policy_returns_not_found_for_nonexistent_policy() {
        // Build engine/store with real mem storage and schema
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = DeletePolicyUseCase::new(store);
        let cmd = DeletePolicyCommand::new("nonexistent_policy_id");
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(DeletePolicyError::NotFound(id)) => {
                assert_eq!(id, "nonexistent_policy_id");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn delete_policy_validates_empty_id() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = DeletePolicyUseCase::new(store);
        let cmd = DeletePolicyCommand::new("");
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(DeletePolicyError::InvalidCommand(_)) => {}
            _ => panic!("Expected InvalidCommand error"),
        }
    }

    #[tokio::test]
    async fn delete_policy_validates_whitespace_only_id() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = DeletePolicyUseCase::new(store);
        let cmd = DeletePolicyCommand::new("   ");
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(DeletePolicyError::InvalidCommand(_)) => {}
            _ => panic!("Expected InvalidCommand error"),
        }
    }
}
</file>

<file path="crates/policies/src/features/get_policy/use_case.rs">
use std::sync::Arc;

use cedar_policy::Policy;

use super::dto::GetPolicyQuery;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum GetPolicyError {
    #[error("invalid_query: {0}")]
    InvalidQuery(String),
    #[error("policy_not_found: {0}")]
    NotFound(String),
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct GetPolicyUseCase {
    store: Arc<PolicyStore>,
}

impl GetPolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(&self, query: &GetPolicyQuery) -> Result<Policy, GetPolicyError> {
        // Validate query
        query
            .validate()
            .map_err(|e| GetPolicyError::InvalidQuery(e.to_string()))?;

        // Get policy from store
        let policy = self
            .store
            .get_policy(&query.policy_id)
            .await
            .map_err(GetPolicyError::Storage)?;

        // Return policy or error if not found
        policy.ok_or_else(|| GetPolicyError::NotFound(query.policy_id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;

    #[tokio::test]
    async fn get_policy_returns_policy_when_exists() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // First, create a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        let policy_id = policy.id().to_string();

        store.add_policy(policy.clone()).await.expect("add policy");

        // Now get it
        let uc = GetPolicyUseCase::new(store);
        let query = GetPolicyQuery::new(policy_id.clone());
        let retrieved = uc.execute(&query).await.expect("get policy");

        assert_eq!(retrieved.id().to_string(), policy_id);
        assert_eq!(retrieved.to_string(), policy.to_string());
    }

    #[tokio::test]
    async fn get_policy_returns_none_when_not_exists() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = GetPolicyUseCase::new(store);
        let query = GetPolicyQuery::new("nonexistent_policy_id");
        let result = uc.execute(&query).await;

        assert!(result.is_err());
        match result {
            Err(GetPolicyError::NotFound(id)) => {
                assert_eq!(id, "nonexistent_policy_id");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn get_policy_validates_empty_id() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = GetPolicyUseCase::new(store);
        let query = GetPolicyQuery::new("");
        let result = uc.execute(&query).await;

        assert!(result.is_err());
        match result {
            Err(GetPolicyError::InvalidQuery(_)) => {}
            _ => panic!("Expected InvalidQuery error"),
        }
    }
}
</file>

<file path="crates/policies/src/features/list_policies/use_case.rs">
use std::sync::Arc;

use cedar_policy::Policy;

use super::dto::ListPoliciesQuery;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum ListPoliciesError {
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct ListPoliciesUseCase {
    store: Arc<PolicyStore>,
}

impl ListPoliciesUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        query: &ListPoliciesQuery,
    ) -> Result<Vec<Policy>, ListPoliciesError> {
        // Validate query
        query
            .validate()
            .map_err(|e| ListPoliciesError::Storage(e.to_string()))?;

        // Get all policies from store
        let policy_set = self
            .store
            .get_current_policy_set()
            .await
            .map_err(|e| ListPoliciesError::Storage(e.to_string()))?;

        // Convert PolicySet to Vec<Policy>
        let mut policies: Vec<Policy> = policy_set.policies().cloned().collect();

        // Apply filter if specified
        if let Some(filter_id) = &query.filter_id {
            policies.retain(|p| p.id().to_string().contains(filter_id));
        }

        // Apply pagination if specified
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(policies.len());

        // Skip offset items and take limit items
        let paginated_policies: Vec<Policy> =
            policies.into_iter().skip(offset).take(limit).collect();

        Ok(paginated_policies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;

    #[tokio::test]
    async fn list_policies_returns_empty_when_no_policies() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = ListPoliciesUseCase::new(store);
        let query = ListPoliciesQuery::new();
        let policies = uc.execute(&query).await.expect("list policies");

        assert_eq!(policies.len(), 0);
    }

    #[tokio::test]
    async fn list_policies_returns_single_policy_after_adding() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // Add a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        let policy_id = policy.id().to_string();
        store.add_policy(policy.clone()).await.expect("add policy");

        // List policies - should have 1
        let uc = ListPoliciesUseCase::new(store);
        let query = ListPoliciesQuery::new();
        let policies = uc.execute(&query).await.expect("list policies");

        assert_eq!(policies.len(), 1, "Expected 1 policy after adding one");
        assert_eq!(policies[0].id().to_string(), policy_id);
        assert_eq!(policies[0].to_string().trim(), policy.to_string().trim());
    }

    #[tokio::test]
    async fn list_policies_works_with_valid_cedar_policies() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // Add a policy with conditions
        let conditional_policy_src = r#"
            permit(
                principal,
                action,
                resource
            ) when {
                principal has email
            };
        "#;
        let conditional_policy: Policy = conditional_policy_src
            .parse()
            .expect("parse conditional policy");
        store
            .add_policy(conditional_policy.clone())
            .await
            .expect("add conditional policy");

        let uc = ListPoliciesUseCase::new(store);
        let query = ListPoliciesQuery::new();
        let policies = uc.execute(&query).await.expect("list policies");

        assert_eq!(policies.len(), 1);
        assert_eq!(
            policies[0].id().to_string(),
            conditional_policy.id().to_string()
        );
    }

    #[tokio::test]
    async fn list_policies_with_pagination() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // Add a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        store.add_policy(policy.clone()).await.expect("add policy");

        let uc = ListPoliciesUseCase::new(store);

        // Test with limit
        let query = ListPoliciesQuery::with_pagination(0, 10);
        let policies = uc.execute(&query).await.expect("list policies");
        assert_eq!(policies.len(), 1);

        // Test with offset that skips all
        let query_skip = ListPoliciesQuery::with_pagination(1, 10);
        let policies_skip = uc
            .execute(&query_skip)
            .await
            .expect("list policies with skip");
        assert_eq!(policies_skip.len(), 0);
    }

    #[tokio::test]
    async fn list_policies_validates_limit() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = ListPoliciesUseCase::new(store);

        // Test with invalid limit (0)
        let query_zero = ListPoliciesQuery::with_pagination(0, 0);
        let result_zero = uc.execute(&query_zero).await;
        assert!(result_zero.is_err());

        // Test with invalid limit (> 1000)
        let query_large = ListPoliciesQuery::with_pagination(0, 1001);
        let result_large = uc.execute(&query_large).await;
        assert!(result_large.is_err());
    }

    #[tokio::test]
    async fn list_policies_with_filter() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // Add a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        let policy_id = policy.id().to_string();
        store.add_policy(policy.clone()).await.expect("add policy");

        let uc = ListPoliciesUseCase::new(store);

        // Test with matching filter
        let query_match = ListPoliciesQuery::with_filter(policy_id.clone());
        let policies_match = uc
            .execute(&query_match)
            .await
            .expect("list policies with filter");
        assert_eq!(policies_match.len(), 1);

        // Test with non-matching filter
        let query_no_match = ListPoliciesQuery::with_filter("nonexistent".to_string());
        let policies_no_match = uc
            .execute(&query_no_match)
            .await
            .expect("list policies with no match");
        assert_eq!(policies_no_match.len(), 0);
    }
}
</file>

<file path="crates/policies/src/features/mod.rs">
// Las features se implementarán según se necesiten
// Por ahora, este módulo está vacío y listo para agregar features
pub mod create_policy;
pub mod get_policy;
pub mod list_policies;
pub mod delete_policy;
pub mod update_policy;
pub mod validate_policy;
pub mod policy_playground;
pub mod policy_playground_traces;
pub mod policy_analysis;
pub mod batch_eval;
</file>

<file path="crates/policies/src/shared/application/di_helpers.rs">
use crate::shared::application::{AuthorizationEngine, EngineBuilder, PolicyStore};
use crate::shared::domain::ports::PolicyStorage;
use crate::shared::infrastructure::surreal::SurrealMemStorage;
use anyhow::Result;
/// Centralized DI helpers to avoid code duplication across features
///
/// This module provides reusable functions for building engines and storage,
/// allowing features to focus on their specific use case construction.
use std::sync::Arc;

#[cfg(feature = "embedded")]
use crate::shared::infrastructure::surreal::SurrealEmbeddedStorage;

/// Build an AuthorizationEngine with a custom EngineBuilder configurator
/// Uses in-memory storage (default dev/test)
pub async fn build_engine_mem<F>(
    configurator: F,
) -> Result<(Arc<AuthorizationEngine>, Arc<PolicyStore>)>
where
    F: FnOnce(EngineBuilder) -> Result<EngineBuilder>,
{
    let storage: Arc<dyn PolicyStorage> =
        Arc::new(SurrealMemStorage::new("policies", "policies").await?);

    let builder = EngineBuilder::new();
    let builder = configurator(builder)?;
    let (engine, store) = builder.build(storage.clone())?;

    Ok((Arc::new(engine), Arc::new(store)))
}

/// Build an AuthorizationEngine with a custom EngineBuilder configurator
/// Uses embedded storage (RocksDB)
#[cfg(feature = "embedded")]
pub async fn build_engine_embedded<F>(
    path: &str,
    configurator: F,
) -> Result<(Arc<AuthorizationEngine>, Arc<PolicyStore>)>
where
    F: FnOnce(EngineBuilder) -> Result<EngineBuilder>,
{
    let storage: Arc<dyn PolicyStorage> =
        Arc::new(SurrealEmbeddedStorage::new("policies", "policies", path).await?);

    let builder = EngineBuilder::new();
    let builder = configurator(builder)?;
    let (engine, store) = builder.build(storage.clone())?;

    Ok((Arc::new(engine), Arc::new(store)))
}

/// No-op configurator - creates an engine with NO entities registered (domain agnostic)
pub fn no_entities_configurator(builder: EngineBuilder) -> Result<EngineBuilder> {
    Ok(builder)
}

/// Test helpers module - provides reusable test entities and configurators
/// Available in both test and non-test builds for integration tests and examples
pub mod test_helpers {
    use super::*;
    use crate::shared::Hrn;
    use crate::shared::domain::ports::{
        Action, AttributeType, HodeiEntity, HodeiEntityType, Principal, Resource,
    };
    use cedar_policy::{EntityTypeName, EntityUid, RestrictedExpression};
    use std::collections::HashMap;
    use std::str::FromStr;

    // Test Principal type
    pub struct TestPrincipal {
        pub hrn: Hrn,
    }

    impl HodeiEntityType for TestPrincipal {
        fn service_name() -> &'static str {
            "test"
        }
        fn resource_type_name() -> &'static str {
            "Principal"
        }
        fn is_principal_type() -> bool {
            true
        }
        fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
            vec![("email", AttributeType::Primitive("String"))]
        }
    }

    impl HodeiEntity for TestPrincipal {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }
        fn attributes(&self) -> HashMap<String, RestrictedExpression> {
            HashMap::new()
        }
        fn parents(&self) -> Vec<EntityUid> {
            Vec::new()
        }
    }

    impl Principal for TestPrincipal {}

    // Test Resource type
    pub struct TestResource {
        pub hrn: Hrn,
    }

    impl HodeiEntityType for TestResource {
        fn service_name() -> &'static str {
            "test"
        }
        fn resource_type_name() -> &'static str {
            "Resource"
        }
        fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
            vec![("name", AttributeType::Primitive("String"))]
        }
    }

    impl HodeiEntity for TestResource {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }
        fn attributes(&self) -> HashMap<String, RestrictedExpression> {
            HashMap::new()
        }
        fn parents(&self) -> Vec<EntityUid> {
            Vec::new()
        }
    }

    impl Resource for TestResource {}

    // Test Action
    pub struct TestAccessAction;

    impl Action for TestAccessAction {
        fn name() -> &'static str {
            "access"
        }
        fn applies_to() -> (EntityTypeName, EntityTypeName) {
            let principal_type =
                EntityTypeName::from_str("Test::Principal").expect("Valid principal type");
            let resource_type =
                EntityTypeName::from_str("Test::Resource").expect("Valid resource type");
            (principal_type, resource_type)
        }
    }

    /// Configurator for tests - registers basic test entities and actions
    pub fn test_entities_configurator(mut builder: EngineBuilder) -> Result<EngineBuilder> {
        builder
            .register_principal::<TestPrincipal>()?
            .register_resource::<TestResource>()?
            .register_action::<TestAccessAction>()?;
        Ok(builder)
    }
}
</file>

<file path="crates/policies/src/shared/domain/hrn.rs">
use cedar_policy::{EntityId, EntityTypeName, EntityUid};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct Hrn {
    pub partition: String,
    pub service: String,
    pub account_id: String,
    pub resource_type: String,
    pub resource_id: String,
}

impl Hrn {
    /// Convención AWS: nombre de servicio siempre en minúsculas (puede contener dígitos y '-')
    pub fn normalize_service_name(service: &str) -> String {
        service.to_ascii_lowercase()
    }

    /// Convierte 'iam' o 'my-service' a 'Iam' o 'MyService' (namespace Cedar)
    pub fn to_pascal_case(s: &str) -> String {
        s.split(['-', '_'])
            .filter(|seg| !seg.is_empty())
            .map(|seg| {
                let mut chars = seg.chars();
                match chars.next() {
                    Some(f) => {
                        f.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn new(
        partition: String,
        service: String,
        account_id: String,
        resource_type: String,
        resource_id: String,
    ) -> Self {
        Self {
            partition,
            service: Self::normalize_service_name(&service),
            account_id,
            resource_type,
            resource_id,
        }
    }

    /// Constructor usando HodeiEntityType para garantizar consistencia
    ///
    /// Este método construye un HRN usando la información del tipo, eliminando
    /// la posibilidad de desincronización entre el esquema y las instancias.
    ///
    /// # Ejemplo
    /// ```ignore
    /// use policies::shared::domain::hrn::Hrn;
    /// use hodei_iam::User; // From hodei-iam crate
    ///
    /// let user_hrn = Hrn::for_entity_type::<User>(
    ///     "hodei".to_string(),
    ///     "default".to_string(),
    ///     "user-123".to_string(),
    /// );
    /// ```
    pub fn for_entity_type<T: crate::shared::domain::ports::HodeiEntityType>(
        partition: String,
        account_id: String,
        resource_id: String,
    ) -> Self {
        Self {
            partition,
            service: Self::normalize_service_name(T::service_name()),
            account_id,
            resource_type: T::resource_type_name().to_string(),
            resource_id,
        }
    }

    pub fn from_string(hrn_str: &str) -> Option<Self> {
        let parts: Vec<&str> = hrn_str.split(':').collect();
        if parts.len() != 6 || parts[0] != "hrn" {
            return None;
        }

        let resource_parts: Vec<&str> = parts[5].splitn(2, '/').collect();
        if resource_parts.len() != 2 {
            return None;
        }

        Some(Hrn {
            partition: parts[1].to_string(),
            service: Self::normalize_service_name(parts[2]),
            account_id: parts[4].to_string(), // El 3er segmento (region) se omite
            resource_type: resource_parts[0].to_string(),
            resource_id: resource_parts[1].to_string(),
        })
    }

    /// Convert HRN to Cedar EntityUid con namespace PascalCase (p.ej., Iam::User)
    ///
    /// Cedar expects UIDs as `Type::"id"`, where Type may be namespaced like `App::User`.
    /// We map:
    /// - Type: if `resource_type` already contains `::`, it's used as-is.
    ///   otherwise, when `service` is non-empty we construct `"{service}::{resource_type}"`.
    ///   both components are normalized to valid Cedar identifiers.
    /// - Id: always quoted string; if parsing fails, we wrap in quotes.
    pub fn euid(&self) -> EntityUid {
        // Namespace Cedar con PascalCase derivado del servicio
        let namespace = Self::to_pascal_case(&self.service);
        let type_str = if self.resource_type.contains("::") {
            self.resource_type.clone()
        } else if !namespace.is_empty() {
            format!(
                "{}::{}",
                namespace,
                Self::normalize_ident(&self.resource_type)
            )
        } else {
            Self::normalize_ident(&self.resource_type)
        };

        let eid = EntityId::from_str(&self.resource_id)
            .or_else(|_| EntityId::from_str(&format!("\"{}\"", self.resource_id)))
            .expect("Failed to create EntityId");
        let type_name =
            EntityTypeName::from_str(&type_str).expect("Failed to create EntityTypeName");
        EntityUid::from_type_name_and_id(type_name, eid)
    }

    /// Normalize a free-form string into a Cedar identifier segment
    /// - first char must be [A-Za-z_]; others may include digits
    /// - non-conforming chars are replaced by '_'
    fn normalize_ident(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        if let Some(c0) = chars.next() {
            let c = if c0.is_ascii_alphabetic() || c0 == '_' {
                c0
            } else {
                '_'
            };
            out.push(c);
        } else {
            out.push('_');
        }
        for c in chars {
            if c.is_ascii_alphanumeric() || c == '_' {
                out.push(c);
            } else {
                out.push('_');
            }
        }
        out
    }

    /// Convenience constructor for Action identifiers. This creates an HRN that
    /// translates into an EntityUid of the form `<service>::Action::"name"` when
    /// `service` is provided, otherwise `Action::"name"`.
    pub fn action(service: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            partition: "aws".to_string(),
            service: Self::normalize_service_name(&service.into()),
            account_id: String::new(),
            resource_type: "Action".to_string(),
            resource_id: name.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display_hrn_roundtrip() {
        let s = "hrn:aws:hodei::123456789012:User/alice";
        let hrn = Hrn::from_string(s).expect("parse hrn");
        assert_eq!(hrn.partition, "aws");
        assert_eq!(hrn.service, "hodei");
        assert_eq!(hrn.account_id, "123456789012");
        assert_eq!(hrn.resource_type, "User");
        assert_eq!(hrn.resource_id, "alice");
        let rendered = hrn.to_string();
        assert!(rendered.contains("User/alice"));
    }

    #[test]
    fn euid_is_constructed() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let euid = hrn.euid();
        // Basic sanity: formatting should include type and id
        let s = format!("{}", euid);
        assert!(s.contains("User"));
        assert!(s.contains("alice"));
    }

    #[test]
    fn euid_uses_service_namespace_for_type() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "hodei-svc".to_string(),
            "123".to_string(),
            "User-Profile".to_string(),
            "bob".to_string(),
        );
        let euid = hrn.euid();
        let s = format!("{}", euid);
        // Expect PascalCase namespace and normalized type (guiones convertidos a guiones bajos)
        assert!(s.contains("HodeiSvc::User_Profile"));
        assert!(s.contains("\"bob\""));
    }

    #[test]
    fn euid_uses_pascal_namespace() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let euid = hrn.euid();
        let s = format!("{}", euid);
        assert!(s.contains("Iam::User"));
        assert!(s.contains("\"alice\""));
    }
}

impl fmt::Display for Hrn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "hrn:{}:{}::{}:{}/{}",
            self.partition, self.service, self.account_id, self.resource_type, self.resource_id
        )
    }
}
</file>

<file path="crates/policies/src/shared/domain/schema_assembler.rs">
//! Typed schema assembler: builds Cedar schema fragments from HodeiEntityType metadata

use crate::shared::HodeiEntityType;
use cedar_policy::{CedarSchemaError, SchemaFragment};
use std::fmt::Write as _;

fn is_lowercase(s: &str) -> bool { s.chars().all(|c| !c.is_ascii_alphabetic() || c.is_ascii_lowercase()) }
fn is_pascal_case(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() { Some(c0) if c0.is_ascii_uppercase() => {}, _ => return false }
    s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn invalid_schema_error() -> Box<CedarSchemaError> {
    // Genera un SchemaError intentando parsear un esquema inválido
    let invalid_schema = "entity Invalid { invalid_attr: InvalidType };";
    match SchemaFragment::from_cedarschema_str(invalid_schema) {
        Err(e) => Box::new(e),
        Ok(_) => {
            // Si por alguna razón el esquema inválido es válido, intentamos con otro
            let conflicting = r#"
                entity Test {};
                entity Test {};
            "#;
            match SchemaFragment::from_cedarschema_str(conflicting) {
                Err(e) => Box::new(e),
                Ok(_) => panic!("Failed to generate a SchemaError"),
            }
        }
    }
}

/// Generate a Cedar SchemaFragment for a given entity type `T`.
///
/// Uses the new service_name() and resource_type_name() methods to construct
/// the fully qualified entity type name (e.g., "IAM::User").
pub fn generate_fragment_for_type<T: HodeiEntityType>() -> Result<SchemaFragment, Box<CedarSchemaError>>
{
    // Validación de convenciones
    let service = T::service_name();
    let resource = T::resource_type_name();
    if !is_lowercase(service) { return Err(invalid_schema_error()); }
    if !is_pascal_case(resource) { return Err(invalid_schema_error()); }

    let attrs = T::cedar_attributes();

    let mut s = String::new();
    
    // Para entidades con namespace, necesitamos declarar el namespace primero
    let namespace = crate::shared::Hrn::to_pascal_case(service);
    let _ = writeln!(s, "namespace {} {{", namespace);
    
    // No usamos "in [Principal]" porque Principal debe estar definido globalmente
    // En su lugar, las entidades principales se identifican por su uso en las acciones

    // entity Header (sin el namespace, ya que estamos dentro del bloque namespace)
    let _ = writeln!(s, "    entity {} {{", resource);

    for (i, (name, atype)) in attrs.iter().enumerate() {
        if i < attrs.len() - 1 {
            let _ = writeln!(s, "        {}: {},", name, atype.to_cedar_decl());
        } else {
            let _ = writeln!(s, "        {}: {}", name, atype.to_cedar_decl());
        }
    }
    // Close entity
    let _ = writeln!(s, "    }};");
    // Close namespace
    let _ = writeln!(s, "}}");

    // Build fragment
    let (frag, _warnings) =
        SchemaFragment::from_cedarschema_str(&s).expect("typed fragment generation should parse");
    Ok(frag)
}
</file>

<file path="crates/policies/tests/principals_schema_test.rs">
use async_trait::async_trait;
use cedar_policy::{EntityUid, RestrictedExpression};
use policies::shared::application::EngineBuilder;
use policies::shared::domain::ports::{
    AttributeType, HodeiEntity, HodeiEntityType, PolicyStorage, Principal, Resource, StorageError,
};
use policies::shared::Hrn;
use std::collections::HashMap;
use std::sync::Arc;

struct DummyStorage;

#[async_trait]
impl PolicyStorage for DummyStorage {
    async fn save_policy(&self, _policy: &cedar_policy::Policy) -> Result<(), StorageError> {
        Ok(())
    }
    async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
        Ok(true)
    }
    async fn get_policy_by_id(
        &self,
        _id: &str,
    ) -> Result<Option<cedar_policy::Policy>, StorageError> {
        Ok(None)
    }
    async fn load_all_policies(&self) -> Result<Vec<cedar_policy::Policy>, StorageError> {
        Ok(vec![])
    }
}

// Tipos de prueba locales (sustituyen a principals::{User, Group} que ahora viven en IAM)
struct TestUser {
    hrn: Hrn,
}

struct TestGroup {
    hrn: Hrn,
}

// Implementación de HodeiEntityType para TestUser
impl HodeiEntityType for TestUser {
    fn service_name() -> &'static str {
        "iam"  // Debe estar en minúsculas según la convención
    }
    fn resource_type_name() -> &'static str {
        "User"
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("email", AttributeType::Primitive("String")),
        ]
    }
    fn is_principal_type() -> bool {
        true
    }
}

// Implementación de HodeiEntity para TestUser
impl HodeiEntity for TestUser {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

// Marker trait Principal para TestUser
impl Principal for TestUser {}

// Implementación de HodeiEntityType para TestGroup
impl HodeiEntityType for TestGroup {
    fn service_name() -> &'static str {
        "iam"  // Debe estar en minúsculas según la convención
    }
    fn resource_type_name() -> &'static str {
        "Group"
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![("name", AttributeType::Primitive("String"))]
    }
}

// Implementación de HodeiEntity para TestGroup
impl HodeiEntity for TestGroup {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

// Marker trait Resource para TestGroup
impl Resource for TestGroup {}

#[tokio::test]
async fn engine_builder_registers_dummy_entities_and_builds() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(DummyStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<TestUser>()
        .expect("register TestUser")
        .register_resource::<TestGroup>()
        .expect("register TestGroup");

    let res = builder.build(storage);
    assert!(res.is_ok(), "engine build should succeed: {:?}", res.err());
}
</file>

<file path="crates/shared/Cargo.toml">
[package]
name = "shared"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
time = { workspace = true }
thiserror = { workspace = true }
cedar-policy = { workspace = true }
uuid = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true, features = ["env-filter", "json"] }
anyhow = { workspace = true }
async-trait = { workspace = true }
tokio = { workspace = true }
bincode = { workspace = true }
chrono = { workspace = true }
surrealdb = { workspace = true }

[dev-dependencies]
tracing-test = { workspace = true }
</file>

<file path="crates/hodei-iam/src/lib.rs">
//! hodei-iam: Default IAM entities for the policies engine
//!
//! This crate provides a standard set of Identity and Access Management entities
//! that can be used with the policies engine. It follows the same Vertical Slice
//! Architecture (VSA) with hexagonal architecture as the policies crate.
//!
//! # Structure
//! - `shared/domain`: Core IAM entities (User, Group, ServiceAccount, Namespace) and actions
//! - `shared/application`: Ports (repository traits) and DI configurator
//! - `shared/infrastructure`: Infrastructure adapters (in-memory repositories for testing)
//! - `features`: IAM-specific features/use cases (create_user, create_group, add_user_to_group)
//!
//! # Example
//! ```no_run
//! use hodei_iam::shared::application::configure_default_iam_entities;
//! use policies::shared::application::di_helpers;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Build an engine with default IAM entities
//! let (engine, store) = di_helpers::build_engine_mem(configure_default_iam_entities).await?;
//! # Ok(())
//! # }
//! ```
pub mod features;
pub mod shared;

// ❌ NO exportar entidades de dominio - son INTERNAS
// Solo se accede a este crate a través de sus casos de uso (features)

// ✅ Re-export features/casos de uso para acceso externo
pub use features::{
    add_user_to_group::AddUserToGroupUseCase,
    create_group::CreateGroupUseCase,
    create_user::CreateUserUseCase,
    get_effective_policies_for_principal::{
        EffectivePoliciesResponse, GetEffectivePoliciesQuery,
        make_use_case as make_get_effective_policies_use_case,
    },
};

use async_trait::async_trait;
use std::sync::Arc;

/// Abstraction (puerto) para consultar políticas efectivas de un principal.
///
/// Este trait elimina la necesidad de que consumidores externos conozcan los parámetros genéricos
/// del caso de uso interno. Se implementa sobre el caso de uso genérico real y se expone
/// como objeto dinámico (`Arc<dyn EffectivePoliciesQueryService>`).
#[async_trait]
pub trait EffectivePoliciesQueryService: Send + Sync {
    async fn get_effective_policies(
        &self,
        query: GetEffectivePoliciesQuery,
    ) -> Result<
        EffectivePoliciesResponse,
        features::get_effective_policies_for_principal::GetEffectivePoliciesError,
    >;
}

/// Implementación del trait para el caso de uso genérico real.
#[async_trait]
impl<UF, GF, PF> EffectivePoliciesQueryService
    for features::get_effective_policies_for_principal::GetEffectivePoliciesForPrincipalUseCase<
        UF,
        GF,
        PF,
    >
where
    UF: features::get_effective_policies_for_principal::UserFinderPort + Send + Sync,
    GF: features::get_effective_policies_for_principal::GroupFinderPort + Send + Sync,
    PF: features::get_effective_policies_for_principal::PolicyFinderPort + Send + Sync,
{
    async fn get_effective_policies(
        &self,
        query: GetEffectivePoliciesQuery,
    ) -> Result<
        EffectivePoliciesResponse,
        features::get_effective_policies_for_principal::GetEffectivePoliciesError,
    > {
        self.execute(query).await
    }
}

/// Tipo de conveniencia para inyección de dependencias.
pub type DynEffectivePoliciesQueryService = Arc<dyn EffectivePoliciesQueryService>;

// ✅ Configurador para policies engine (necesario para setup inicial)
pub use shared::application::configure_default_iam_entities;
</file>

<file path="crates/policies/src/features/create_policy/use_case.rs">
use std::sync::Arc;

use cedar_policy::Policy;

use super::dto::CreatePolicyCommand;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum CreatePolicyError {
    #[error("invalid_policy: {0}")]
    InvalidPolicy(String),
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct CreatePolicyUseCase {
    store: Arc<PolicyStore>,
}

impl CreatePolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(&self, cmd: &CreatePolicyCommand) -> Result<(), CreatePolicyError> {
        // Validate command
        cmd.validate()
            .map_err(|e| CreatePolicyError::InvalidPolicy(e.to_string()))?;

        let policy: Policy = cmd
            .policy_src
            .parse::<Policy>()
            .map_err(|e| CreatePolicyError::InvalidPolicy(e.to_string()))?;
        self.store
            .add_policy(policy)
            .await
            .map_err(CreatePolicyError::Storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;

    #[tokio::test]
    async fn create_policy_persists_in_surreal_mem() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = CreatePolicyUseCase::new(store);
        let cmd = crate::features::create_policy::dto::CreatePolicyCommand::new(
            r#"permit(principal, action, resource);"#,
        );
        uc.execute(&cmd).await.expect("create policy");

        // Ensure it's in the current set
        let pset = engine
            .store
            .get_current_policy_set()
            .await
            .expect("policy set");
        assert!(pset.to_cedar().is_some());
    }
}
</file>

<file path="crates/policies/src/features/delete_policy/di.rs">
use super::use_case::DeletePolicyUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build DeletePolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: For tests, this uses test entities. For production, use hodei-iam::di or register your own entities.
pub async fn make_delete_policy_use_case_mem() -> Result<(DeletePolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = DeletePolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build DeletePolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: For tests, this uses test entities. For production, use hodei-iam::di or register your own entities.
    pub async fn make_delete_policy_use_case_embedded(
        path: &str,
    ) -> Result<(DeletePolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = DeletePolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/features/get_policy/di.rs">
use super::use_case::GetPolicyUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build GetPolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: This creates an engine with NO entities registered.
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_get_policy_use_case_mem() -> Result<(GetPolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = GetPolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build GetPolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_get_policy_use_case_embedded(
        path: &str,
    ) -> Result<(GetPolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = GetPolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/features/list_policies/di.rs">
use super::use_case::ListPoliciesUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build ListPoliciesUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: This creates an engine with NO entities registered.
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_list_policies_use_case_mem() -> Result<(ListPoliciesUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = ListPoliciesUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build ListPoliciesUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_list_policies_use_case_embedded(
        path: &str,
    ) -> Result<(ListPoliciesUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = ListPoliciesUseCase::new(store);
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/features/update_policy/di.rs">
use super::use_case::UpdatePolicyUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build UpdatePolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: This creates an engine with NO entities registered.
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_update_policy_use_case_mem() -> Result<(UpdatePolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = UpdatePolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build UpdatePolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_update_policy_use_case_embedded(
        path: &str,
    ) -> Result<(UpdatePolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = UpdatePolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/features/update_policy/use_case.rs">
use std::sync::Arc;

use cedar_policy::Policy;

use super::dto::UpdatePolicyCommand;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum UpdatePolicyError {
    #[error("invalid_command: {0}")]
    InvalidCommand(String),
    #[error("policy_not_found: {0}")]
    NotFound(String),
    #[error("policy_parse_error: {0}")]
    ParseError(String),
    #[error("validation_error: {0}")]
    ValidationError(String),
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct UpdatePolicyUseCase {
    store: Arc<PolicyStore>,
}

impl UpdatePolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        cmd: &UpdatePolicyCommand,
    ) -> Result<Policy, UpdatePolicyError> {
        // 1. Validar comando
        cmd.validate()
            .map_err(|e| UpdatePolicyError::InvalidCommand(e.to_string()))?;

        // 2. Verificar que la política existe
        let existing = self
            .store
            .get_policy(&cmd.policy_id)
            .await
            .map_err(UpdatePolicyError::Storage)?;

        if existing.is_none() {
            return Err(UpdatePolicyError::NotFound(cmd.policy_id.clone()));
        }

        // 3. Parsear nueva política
        let new_policy: Policy = cmd
            .new_policy_content
            .parse()
            .map_err(|e| UpdatePolicyError::ParseError(format!("{}", e)))?;

        // 4. Actualizar política (esto valida automáticamente)
        self.store
            .update_policy(&cmd.policy_id, new_policy.clone())
            .await
            .map_err(UpdatePolicyError::ValidationError)?;

        Ok(new_policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;

    #[tokio::test]
    async fn update_policy_successfully_updates_existing_policy() {
        // Arrange: Create engine/store and add a policy (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // Add original policy
        let original_policy_src = r#"permit(principal, action, resource);"#;
        let original_policy: Policy = original_policy_src.parse().expect("parse original");
        let policy_id = original_policy.id().to_string();
        store
            .add_policy(original_policy.clone())
            .await
            .expect("add original policy");

        // Act: Update the policy
        let uc = UpdatePolicyUseCase::new(store.clone());
        let new_content = r#"forbid(principal, action, resource);"#;
        let cmd = UpdatePolicyCommand::new(policy_id.clone(), new_content.to_string());
        let result = uc.execute(&cmd).await;

        // Assert: Should succeed
        assert!(result.is_ok());
        let updated_policy = result.unwrap();
        assert_eq!(updated_policy.to_string().trim(), new_content.trim());

        // Verify the policy was actually updated in storage
        let retrieved = store.get_policy(&policy_id).await.expect("get policy");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().to_string().trim(), new_content.trim());
    }

    #[tokio::test]
    async fn update_policy_returns_not_found_for_nonexistent_policy() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = UpdatePolicyUseCase::new(store);
        let cmd = UpdatePolicyCommand::new(
            "nonexistent_policy_id".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(UpdatePolicyError::NotFound(id)) => {
                assert_eq!(id, "nonexistent_policy_id");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn update_policy_validates_empty_id() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = UpdatePolicyUseCase::new(store);
        let cmd = UpdatePolicyCommand::new(
            "".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(UpdatePolicyError::InvalidCommand(_)) => {}
            _ => panic!("Expected InvalidCommand error"),
        }
    }

    #[tokio::test]
    async fn update_policy_validates_empty_content() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = UpdatePolicyUseCase::new(store);
        let cmd = UpdatePolicyCommand::new("policy_id".to_string(), "".to_string());
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(UpdatePolicyError::InvalidCommand(_)) => {}
            _ => panic!("Expected InvalidCommand error"),
        }
    }

    #[tokio::test]
    async fn update_policy_validates_new_policy_syntax() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // Add original policy
        let original_policy_src = r#"permit(principal, action, resource);"#;
        let original_policy: Policy = original_policy_src.parse().expect("parse original");
        let policy_id = original_policy.id().to_string();
        store
            .add_policy(original_policy.clone())
            .await
            .expect("add original policy");

        let uc = UpdatePolicyUseCase::new(store);
        let cmd = UpdatePolicyCommand::new(
            policy_id,
            "this is not valid cedar syntax".to_string(),
        );
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(UpdatePolicyError::ParseError(_)) => {}
            _ => panic!("Expected ParseError"),
        }
    }
}
</file>

<file path="crates/policies/src/shared/domain/mod.rs">
// Local modules in shared/domain
pub mod entity_utils;
pub mod error;
pub mod hrn;
pub mod ports;
pub mod schema_assembler;

// Convenience re-exports for external use
pub use error::HodeiPoliciesError;
pub use hrn::Hrn;
pub use ports::{Action, AttributeType, HodeiEntity, HodeiEntityType, Principal, Resource};
</file>

<file path="crates/policies/src/shared/mod.rs">
// Facade raíz del crate policies (estructura hexagonal interna)
pub mod application;
pub mod domain;
pub mod infrastructure;

// Re-exports para tests e integración
pub use application::{AuthorizationEngine, AuthorizationRequest, EngineBuilder, PolicyStore};
pub use domain::{
    entity_utils,
    hrn::Hrn,
    ports::{Action, AttributeType, HodeiEntity, HodeiEntityType, Principal, Resource},
    schema_assembler::*,
};

// Re-exports de Cedar comunes en tests
pub use cedar_policy::{Context, EntityUid, Policy, PolicyId};
</file>

<file path="crates/policies/Cargo.toml">
[package]
name = "policies"
version = "0.1.0"
edition = "2024"
license = "MIT"

[dependencies]
# Core dependencies
serde = { workspace = true, features = ["derive"] }
thiserror = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
shared = { path = "../shared" }
chrono = { workspace = true }
serde_json = { workspace = true }
sha2 = { workspace = true }

# Cedar Policy Engine
cedar-policy = { workspace = true }

# Database - SurrealDB for policy storage (backend via crate features)
surrealdb = { workspace = true }

# Async runtime
tokio = { workspace = true, features = ["full"] }
async-trait = { workspace = true }

## testing dependencies are only declared under [dev-dependencies]

[features]
default = ["mem"]
mem = ["surrealdb/kv-mem"]
embedded = ["surrealdb/kv-rocksdb"]
integration = []

[dev-dependencies]
mockall = { workspace = true }
testcontainers = { workspace = true }
futures = { workspace = true }
uuid = { workspace = true }
regex = { workspace = true }
</file>

<file path="crates/shared/src/models.rs">
// crates/shared/src/models.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::enums::HashAlgorithm;

/// El hash criptográfico del contenido de un fichero físico. Es inmutable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash {
    /// El algoritmo utilizado para generar el hash (ej. Sha256).
    pub algorithm: HashAlgorithm,
    /// El valor del hash en formato hexadecimal.
    pub value: String,
}

/// Coordenadas universales que identifican un paquete en cualquier ecosistema.
/// No contiene el ecosistema, ya que este se infiere del `Repository` contenedor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageCoordinates {
    /// El espacio de nombres del paquete (ej. `@scope` en npm, `groupId` en Maven).
    pub namespace: Option<String>,
    /// El nombre del paquete (ej. `react`, `log4j-core`).
    pub name: String,
    /// La versión del paquete (ej. "18.2.0", "2.17.1").
    pub version: String,
    /// Pares clave-valor para metadatos específicos del ecosistema que son necesarios para la identificación
    /// (ej. `classifier="sources"` en Maven, `os="linux"` en OCI).
    pub qualifiers: HashMap<String, String>,
}

impl PackageCoordinates {
    pub fn new(namespace: &str, name: &str, version: &str) -> Self {
        Self::with_qualifiers(namespace, name, version, HashMap::new())
    }

    pub fn with_qualifiers(
        namespace: &str,
        name: &str,
        version: &str,
        qualifiers: HashMap<String, String>,
    ) -> Self {
        Self {
            namespace: if namespace.is_empty() {
                None
            } else {
                Some(namespace.to_string())
            },
            name: name.to_string(),
            version: version.to_string(),
            qualifiers,
        }
    }
}

/// Referencia a un artefacto físico, alineada con el diagrama de dominio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactReference {
    /// HRN del artefacto físico.
    pub physical_artifact_hrn: String,
    /// Tamaño del artefacto en bytes.
    pub size_in_bytes: u64,
    /// Hash del contenido del artefacto.
    pub content_hash: ContentHash,
}
</file>

<file path="crates/policies/src/features/create_policy/di.rs">
use super::use_case::CreatePolicyUseCase;
use crate::shared::application::{di_helpers, AuthorizationEngine};
use anyhow::Result;
use std::sync::Arc;

/// Build CreatePolicyUseCase wired with SurrealDB in-memory storage (default dev/test)
/// NOTE: This creates an engine with NO entities registered. 
/// Consumers should use hodei-iam::di or register their own entities.
pub async fn make_create_policy_use_case_mem() -> Result<(CreatePolicyUseCase, Arc<AuthorizationEngine>)> {
    let (engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator).await?;
    let uc = CreatePolicyUseCase::new(store);
    Ok((uc, engine))
}

#[cfg(feature = "embedded")]
pub mod embedded {
    use super::*;
    use crate::shared::application::di_helpers;

    /// Build CreatePolicyUseCase wired with SurrealDB embedded (RocksDB)
    /// NOTE: This creates an engine with NO entities registered.
    /// Consumers should use hodei-iam::di or register their own entities.
    pub async fn make_create_policy_use_case_embedded(
        path: &str,
    ) -> Result<(CreatePolicyUseCase, Arc<AuthorizationEngine>)> {
        let (engine, store) = di_helpers::build_engine_embedded(path, di_helpers::test_helpers::test_entities_configurator).await?;
        let uc = CreatePolicyUseCase::new(store);
        Ok((uc, engine))
    }
}
</file>

<file path="crates/policies/src/shared/application/engine.rs">
use crate::shared::application::PolicyStore;
use crate::shared::domain::HodeiEntity;
use crate::shared::domain::ports::PolicyStorage;
use crate::shared::domain::ports::{Action, Principal, Resource};
use crate::shared::generate_fragment_for_type;
use cedar_policy::{
    CedarSchemaError, Context, Entities, PolicySet, Request, Response, Schema, SchemaError,
    SchemaFragment,
};
use std::collections::HashSet;
use std::sync::Arc;

pub struct AuthorizationRequest<'a> {
    pub principal: &'a dyn HodeiEntity,
    pub action: cedar_policy::EntityUid,
    pub resource: &'a dyn HodeiEntity,
    pub context: Context,
    pub entities: Vec<&'a dyn HodeiEntity>,
}

#[derive(Clone)]
pub struct AuthorizationEngine {
    pub schema: Arc<Schema>,
    pub store: PolicyStore,
}

impl AuthorizationEngine {
    /// Evaluate authorization using the internal PolicyStore
    pub async fn is_authorized(&self, request: &AuthorizationRequest<'_>) -> Response {
        let entity_vec: Vec<cedar_policy::Entity> = request
            .entities
            .iter()
            .map(|entity| {
                let attrs = entity.attributes();
                let parents: HashSet<_> = entity.parents().into_iter().collect();
                cedar_policy::Entity::new(entity.euid(), attrs, parents)
            })
            .collect::<Result<Vec<_>, _>>()
            .expect("Failed to create entities");

        let entities = Entities::from_entities(entity_vec, None)
            .expect("Failed to create Entities collection");

        let cedar_request = Request::new(
            request.principal.euid(),
            request.action.clone(),
            request.resource.euid(),
            request.context.clone(),
            None,
        )
        .expect("Failed to create Cedar request");

        let policies = self
            .store
            .get_current_policy_set()
            .await
            .unwrap_or_else(|_| PolicySet::new());
        cedar_policy::Authorizer::new().is_authorized(&cedar_request, &policies, &entities)
    }

    /// Evaluate authorization using an external PolicySet
    ///
    /// This method allows orchestrators (like hodei-authorizer) to provide
    /// a dynamically constructed PolicySet without requiring PolicyStore persistence.
    ///
    /// # Use Case
    /// Use this when policies are collected from multiple sources at runtime
    /// (e.g., IAM policies + SCPs) and need to be evaluated together.
    pub fn is_authorized_with_policy_set(
        &self,
        request: &AuthorizationRequest<'_>,
        policies: &PolicySet,
    ) -> Response {
        let entity_vec: Vec<cedar_policy::Entity> = request
            .entities
            .iter()
            .map(|entity| {
                let attrs = entity.attributes();
                let parents: HashSet<_> = entity.parents().into_iter().collect();
                cedar_policy::Entity::new(entity.euid(), attrs, parents)
            })
            .collect::<Result<Vec<_>, _>>()
            .expect("Failed to create entities");

        let entities = Entities::from_entities(entity_vec, None)
            .expect("Failed to create Entities collection");

        let cedar_request = Request::new(
            request.principal.euid(),
            request.action.clone(),
            request.resource.euid(),
            request.context.clone(),
            None,
        )
        .expect("Failed to create Cedar request");

        cedar_policy::Authorizer::new().is_authorized(&cedar_request, policies, &entities)
    }
}

#[derive(Default)]
pub struct EngineBuilder {
    entity_fragments: Vec<SchemaFragment>,
    action_fragments: Vec<SchemaFragment>,
}

impl EngineBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    // New methods for the generic approach
    pub fn register_principal<P: Principal>(&mut self) -> Result<&mut Self, Box<CedarSchemaError>> {
        let frag = generate_fragment_for_type::<P>()?;
        self.entity_fragments.push(frag);
        Ok(self)
    }

    pub fn register_resource<R: Resource>(&mut self) -> Result<&mut Self, Box<CedarSchemaError>> {
        let frag = generate_fragment_for_type::<R>()?;
        self.entity_fragments.push(frag);
        Ok(self)
    }

    pub fn register_action<A: Action>(&mut self) -> Result<&mut Self, Box<CedarSchemaError>> {
        let (principal_type, resource_type) = A::applies_to();
        let schema_str = format!(
            "action \"{}\" appliesTo {{ principal: {}, resource: {} }};",
            A::name(),
            principal_type,
            resource_type
        );

        // Parse the action schema fragment
        let (frag, _warnings) =
            SchemaFragment::from_cedarschema_str(&schema_str).map_err(|_e| {
                // If parsing fails, create a SchemaError by parsing an intentionally invalid schema
                // This ensures we return the correct error type
                let invalid = "entity Invalid { invalid: Invalid }";
                match SchemaFragment::from_cedarschema_str(invalid) {
                    Ok(_) => unreachable!(),
                    Err(_cedar_err) => {
                        // Create a generic schema parsing error using Schema::from_schema_fragments
                        // with an empty fragment list to trigger a schema error
                        Box::new(CedarSchemaError::from(
                            Schema::from_schema_fragments(vec![]).unwrap_err(),
                        ))
                    }
                }
            })?;

        self.action_fragments.push(frag);
        Ok(self)
    }

    pub fn build(
        self,
        storage: Arc<dyn PolicyStorage>,
    ) -> Result<(AuthorizationEngine, PolicyStore), Box<SchemaError>> {
        // Build schema from registered fragments only
        // No automatic base schema - everything must be explicitly registered by the client
        let all_fragments = [self.entity_fragments, self.action_fragments].concat();

        let schema = Arc::new(Schema::from_schema_fragments(all_fragments)?);
        let store = PolicyStore::new(schema.clone(), storage);
        let engine = AuthorizationEngine {
            schema,
            store: store.clone(),
        };
        Ok((engine, store))
    }
}
</file>

<file path="crates/shared/src/lib.rs">
// crates/shared/src/lib.rs

pub mod enums;
// pub mod events;  // Temporalmente desactivado - depende de Hrn
// pub mod lifecycle;  // Temporalmente desactivado - depende de Hrn
pub mod application;
pub mod infrastructure;
pub mod models;


// Re-export application types for ergonomic use
pub use application::{UnitOfWork, UnitOfWorkError, UnitOfWorkFactory};

// Re-export event bus types for ergonomic use
pub use application::ports::{
    DomainEvent, EventBus, EventEnvelope, EventHandler, EventPublisher, Subscription,
};

// Re-export infrastructure implementations
pub use infrastructure::InMemoryEventBus;
</file>

</files>
