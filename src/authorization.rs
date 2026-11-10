//! Shared authorization wiring for Artifacts.
//!
//! The application owns a small [`AuthorizerPort`] and a real
//! [`AuthorizeResourceUseCase`]. The port can be backed by the shared
//! `hodei-authz` authorizer or by the compatibility deny-all implementation
//! while a policy bundle is not wired. The use case never reads HTTP headers:
//! it receives an already verified [`VerifiedContext`].

use std::sync::Arc;

use hodei_authz::hodei_hrn::Hrn;
use hodei_authz::{
    ActionId, AuthorizationOutcome, AuthorizationRequest, Authorizer, DenyReason, ResourceRef,
    VerifiedContext,
};
use kernel::Hrn as CanonicalHrn;

/// Small application port consumed by Artifacts use cases.
pub trait AuthorizerPort: Send + Sync {
    fn authorize(&self, request: &AuthorizationRequest) -> AuthorizationOutcome;
}

/// Adapter from the shared `hodei-authz` authorizer.
pub struct SharedAuthorizer {
    inner: Arc<dyn Authorizer>,
}

impl SharedAuthorizer {
    pub fn new(inner: Arc<dyn Authorizer>) -> Self {
        Self { inner }
    }
}

impl AuthorizerPort for SharedAuthorizer {
    fn authorize(&self, request: &AuthorizationRequest) -> AuthorizationOutcome {
        self.inner.decide(request)
    }
}

/// Compatibility implementation used until a policy bundle is injected.
///
/// It denies every request; it is the reversible fallback for
/// [`super::composition_root::CompositionRoot::with_authorization`].
pub struct DenyAllAuthorizer;

impl AuthorizerPort for DenyAllAuthorizer {
    fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
        AuthorizationOutcome::deny(DenyReason::NoMatchingPolicy)
    }
}

/// Real use case: authorize an action on a scoped resource for a verified
/// principal.
///
/// The account scope is checked before the policy engine. A cross-account
/// request cannot become authorized by changing an HTTP header because the
/// command has no header input: it receives the verified context and a typed
/// resource.
pub struct AuthorizeResourceUseCase {
    authorizer: Arc<dyn AuthorizerPort>,
}

impl AuthorizeResourceUseCase {
    pub fn new(authorizer: Arc<dyn AuthorizerPort>) -> Self {
        Self { authorizer }
    }

    /// The first argument is a [`VerifiedContext`], never a principal string
    /// or an HTTP header.
    ///
    /// ```compile_fail
    /// # use hodei_artifacts_api::authorization::AuthorizeResourceUseCase;
    /// # fn example(
    /// #     use_case: AuthorizeResourceUseCase,
    /// #     action: hodei_authz::ActionId,
    /// #     resource: hodei_authz::ResourceRef,
    /// # ) {
    /// let _ = use_case.execute("hrn:hodei:iam::acme:User/attacker", action, resource);
    /// # }
    /// ```
    pub fn execute(
        &self,
        context: VerifiedContext,
        action: ActionId,
        resource: ResourceRef,
    ) -> AuthorizationOutcome {
        let request = AuthorizationRequest::new(context, action, resource);
        let principal_hrn: Hrn = match request.principal().subject().as_str().parse() {
            Ok(hrn) => hrn,
            Err(_) => {
                return AuthorizationOutcome::deny(DenyReason::ScopeMismatch);
            }
        };
        if !request.is_same_account()
            || principal_hrn.partition != "hodei"
            || request.resource().hrn().partition != "hodei"
            || principal_hrn.partition != request.resource().hrn().partition
        {
            return AuthorizationOutcome::deny(DenyReason::ScopeMismatch);
        }
        self.authorizer.authorize(&request)
    }

    /// Execute with the canonical Artifacts HRN.
    ///
    /// The kernel HRN is converted field by field so nested resource paths
    /// (`repository/releases/v2`) are preserved instead of being re-parsed by
    /// the legacy `hodei-hrn` 0.1 parser, which only accepts two resource
    /// segments.
    pub fn execute_canonical(
        &self,
        context: VerifiedContext,
        action: ActionId,
        hrn: &CanonicalHrn,
    ) -> Result<AuthorizationOutcome, hodei_authz::ModelError> {
        let authz_hrn = Hrn {
            partition: hrn.partition.clone(),
            service: hrn.service.clone(),
            region: hrn.region.clone(),
            tenant_id: hrn.account_id.clone(),
            resource_type: hrn.resource_type.clone(),
            resource_id: hrn.resource_id.clone(),
        };
        let resource = ResourceRef::new(authz_hrn)?;
        Ok(self.execute(context, action, resource))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use cedar_policy::Entities;
    use hodei_authz::cedar::CedarAuthorizer;
    use hodei_authz::hodei_hrn::Hrn;
    use hodei_authz::hrn_mapper::HrnCedarMapper;
    use hodei_authz::{
        AccountId, AuthenticationError, Credentials, IndeterminateReason, PrincipalAuthenticator,
        PrincipalClaims, PrincipalId, verify_principal,
    };

    struct TestAuthenticator;

    #[async_trait]
    impl PrincipalAuthenticator for TestAuthenticator {
        async fn authenticate(
            &self,
            _credentials: &Credentials,
        ) -> Result<PrincipalClaims, AuthenticationError> {
            Ok(PrincipalClaims::new(
                PrincipalId::new("hrn:hodei:iam::acme:User/alice").unwrap(),
                AccountId::new("acme").unwrap(),
            ))
        }
    }

    async fn context() -> VerifiedContext {
        VerifiedContext::from_principal(
            verify_principal(&TestAuthenticator, &Credentials::bearer("token"))
                .await
                .unwrap(),
        )
    }

    fn resource(account: &str) -> ResourceRef {
        let hrn: Hrn = format!("hrn:hodei:artifacts:eu:{account}:repository/releases")
            .parse()
            .unwrap();
        ResourceRef::new(hrn).unwrap()
    }

    fn action() -> ActionId {
        ActionId::new("artifacts", "ReadRepository").unwrap()
    }

    fn cedar(policy: &str) -> Arc<dyn AuthorizerPort> {
        Arc::new(SharedAuthorizer::new(Arc::new(
            CedarAuthorizer::from_policies_unchecked(
                policy,
                Entities::empty(),
                Arc::new(HrnCedarMapper),
            )
            .unwrap(),
        )))
    }

    #[tokio::test]
    async fn permit_is_returned_by_the_real_use_case() {
        let policy = r#"permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"ReadRepository",
            resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/releases"
        );"#;
        let use_case = AuthorizeResourceUseCase::new(cedar(policy));
        assert_eq!(
            use_case.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Permit
        );
    }

    #[tokio::test]
    async fn deny_and_indeterminate_are_preserved() {
        let deny = AuthorizeResourceUseCase::new(Arc::new(DenyAllAuthorizer));
        assert_eq!(
            deny.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
        );

        let policy = r#"permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"ReadRepository",
            resource
        ) when { principal.role == "admin" };"#;
        let use_case = AuthorizeResourceUseCase::new(cedar(policy));
        assert_eq!(
            use_case.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Indeterminate(IndeterminateReason::EvaluationError)
        );
    }

    struct CountingAuthorizer {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl AuthorizerPort for CountingAuthorizer {
        fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            AuthorizationOutcome::Permit
        }
    }

    #[tokio::test]
    async fn cross_account_is_denied_before_the_engine_and_the_port_is_not_called() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let use_case = AuthorizeResourceUseCase::new(Arc::new(CountingAuthorizer {
            calls: calls.clone(),
        }));
        assert_eq!(
            use_case.execute(context().await, action(), resource("other")),
            AuthorizationOutcome::Deny(DenyReason::ScopeMismatch)
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_partition_change_is_also_a_scope_mismatch() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let use_case = AuthorizeResourceUseCase::new(Arc::new(CountingAuthorizer {
            calls: calls.clone(),
        }));
        let hrn: Hrn = "hrn:evil:artifacts:eu:acme:repository/releases"
            .parse()
            .unwrap();
        let foreign_partition = ResourceRef::new(hrn).unwrap();

        assert_eq!(
            use_case.execute(context().await, action(), foreign_partition),
            AuthorizationOutcome::Deny(DenyReason::ScopeMismatch)
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn canonical_hrn_with_nested_path_reaches_the_shared_authorizer() {
        let policy = r#"permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"ReadRepository",
            resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/releases/v2"
        );"#;
        let use_case = AuthorizeResourceUseCase::new(cedar(policy));
        let canonical =
            CanonicalHrn::from_string("hrn:hodei:artifacts:eu:acme:repository/releases/v2")
                .expect("canonical HRN");

        assert_eq!(
            use_case
                .execute_canonical(context().await, action(), &canonical)
                .expect("canonical resource"),
            AuthorizationOutcome::Permit
        );
    }

    #[tokio::test]
    async fn compatibility_adapter_is_reversible() {
        let policy = r#"permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"ReadRepository",
            resource
        );"#;
        let shared = AuthorizeResourceUseCase::new(cedar(policy));
        let fallback = AuthorizeResourceUseCase::new(Arc::new(DenyAllAuthorizer));

        assert_eq!(
            shared.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Permit
        );
        assert_eq!(
            fallback.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
        );
    }

    #[test]
    fn the_port_is_object_safe_and_send_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn AuthorizerPort>();
        let _ = BTreeMap::<String, String>::new();
    }
}
