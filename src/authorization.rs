//! Shared authorization wiring for Artifacts.
//!
//! The application owns a small [`AuthorizerPort`] and a real
//! [`AuthorizeResourceUseCase`]. The port can be backed by the shared
//! `hodei-authz` authorizer or by the compatibility deny-all implementation
//! while a policy bundle is not wired. The use case never reads HTTP headers:
//! it receives an already verified [`VerifiedContext`].
//!
//! HSP-150 adds the ABAC layers on top of the port: [`ResourceScope`] carries
//! the trusted namespace and tags of repository and package resources,
//! [`BoundaryAuthorizer`] intersects an identity/resource policy layer with
//! an explicit permission boundary, and [`CachedAuthorizer`] memoizes
//! decisions per principal, resource, policy generation and attribute
//! fingerprint so a policy or label change never reuses another principal's
//! decision.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hodei_authz::hodei_hrn::Hrn;
use hodei_authz::{
    ActionId, AttributeValue, Attributes, AuthorizationOutcome, AuthorizationRequest, Authorizer,
    DenyReason, ResourceRef, VerifiedContext,
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

/// The attribute names the resource scopes export into Cedar policies.
///
/// Policies refer to `resource.namespace` and `resource.tags`; keeping the
/// names in constants ties the trusted producer and the policy text together.
pub const NAMESPACE_ATTRIBUTE: &str = "namespace";
/// The tag-set attribute name; see [`NAMESPACE_ATTRIBUTE`].
pub const TAGS_ATTRIBUTE: &str = "tags";

/// Why a namespace or tag failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeValidationError(&'static str);

impl std::fmt::Display for ScopeValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid resource scope: {}", self.0)
    }
}

impl std::error::Error for ScopeValidationError {}

/// Trusted namespace of a repository or package resource (HSP-150).
///
/// Validated lowercase dotted segments; it is built by the application from
/// server-side metadata, never from a raw request header.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Namespace(String);

impl Namespace {
    /// Validate and construct a namespace (`com.example`, `npm`, `crates-io`).
    pub fn parse(value: impl Into<String>) -> Result<Self, ScopeValidationError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ScopeValidationError("empty namespace"));
        }
        if value.len() > 253 {
            return Err(ScopeValidationError("namespace longer than 253 bytes"));
        }
        for segment in value.split('.') {
            if segment.is_empty() {
                return Err(ScopeValidationError("empty namespace segment"));
            }
            if segment.len() > 63 {
                return Err(ScopeValidationError(
                    "namespace segment longer than 63 bytes",
                ));
            }
            if !segment.starts_with(|c: char| c.is_ascii_alphanumeric())
                || !segment.ends_with(|c: char| c.is_ascii_alphanumeric())
                || !segment
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                return Err(ScopeValidationError(
                    "namespace segment is not lowercase alphanumeric",
                ));
            }
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Trusted tag of a repository or package resource (HSP-150).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceTag(String);

impl ResourceTag {
    /// Validate and construct a tag; the value set is a documented naming
    /// convention but the charset is enforced so tags survive as Cedar strings.
    pub fn parse(value: impl Into<String>) -> Result<Self, ScopeValidationError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ScopeValidationError("empty tag"));
        }
        if value.len() > 128 {
            return Err(ScopeValidationError("tag longer than 128 bytes"));
        }
        if !value.starts_with(|c: char| c.is_ascii_alphanumeric())
            || !value.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.' | ':')
            })
        {
            return Err(ScopeValidationError(
                "tag is not a lowercase alphanumeric slug",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The trusted ABAC scope of a resource: namespace plus verified tags.
///
/// The producer is the application layer working from validated repository
/// metadata; the use case has already refused any request whose principal is
/// not a [`VerifiedContext`]. Converting the scope into a Cedar-backed
/// [`ResourceRef`] attaches the attributes the policies read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceScope {
    namespace: Namespace,
    tags: BTreeSet<ResourceTag>,
}

impl ResourceScope {
    pub fn new(namespace: Namespace) -> Self {
        Self {
            namespace,
            tags: BTreeSet::new(),
        }
    }

    #[must_use]
    pub fn with_tag(mut self, tag: ResourceTag) -> Self {
        self.tags.insert(tag);
        self
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn tags(&self) -> &BTreeSet<ResourceTag> {
        &self.tags
    }

    /// Cedar-attribute view of the scope: a `namespace` string and a `tags`
    /// set, the only attributes this layer trusts on a resource.
    pub fn trusted_attributes(&self) -> Attributes {
        let mut attributes = Attributes::new();
        attributes.insert(
            NAMESPACE_ATTRIBUTE.to_string(),
            AttributeValue::String(self.namespace.as_str().to_string()),
        );
        attributes.insert(
            TAGS_ATTRIBUTE.to_string(),
            AttributeValue::Set(
                self.tags
                    .iter()
                    .map(|tag| tag.as_str().to_string())
                    .collect::<BTreeSet<_>>(),
            ),
        );
        attributes
    }

    /// Attach the trusted attributes to a typed resource reference.
    pub fn apply(self, resource: ResourceRef) -> ResourceRef {
        let mut attributes = resource.attributes().clone();
        attributes.extend(self.trusted_attributes());
        resource.with_attributes(attributes)
    }
}

/// The key under `config_settings` that holds the tags of a repository.
pub const REPOSITORY_TAGS_SETTING: &str = "tags";

/// The most tags a repository can carry.
const MAX_REPOSITORY_TAGS: usize = 64;

/// Parse the tags a repository declares in its settings.
///
/// An absent key is "no tags". Anything that is not an array of valid
/// [`ResourceTag`]s is an error, never silently dropped: a dropped tag would
/// change who a tag-based policy admits.
pub fn tags_of_settings(
    settings: &serde_json::Value,
) -> Result<BTreeSet<ResourceTag>, ScopeValidationError> {
    let Some(value) = settings.get(REPOSITORY_TAGS_SETTING) else {
        return Ok(BTreeSet::new());
    };
    let items = value
        .as_array()
        .ok_or(ScopeValidationError("tags are not an array"))?;
    if items.len() > MAX_REPOSITORY_TAGS {
        return Err(ScopeValidationError("too many tags"));
    }
    items
        .iter()
        .map(|item| {
            item.as_str()
                .ok_or(ScopeValidationError("a tag is not a string"))
                .and_then(ResourceTag::parse)
        })
        .collect()
}

/// The trusted [`ResourceScope`] of a repository (HSP-321).
///
/// The namespace is the owning account, lowercased with `_` mapped to `-` so
/// it fits the namespace grammar; accounts are already isolated by the
/// use case (`ScopeMismatch`), so two accounts that normalize to the same
/// namespace can never reach each other. The repository itself is identified
/// by its HRN. Tags come from the validated `config_settings.tags` section.
/// Any failure is an error that callers must treat as `Indeterminate`.
pub fn repository_scope(
    account: &str,
    settings: &serde_json::Value,
) -> Result<ResourceScope, ScopeValidationError> {
    let namespace = Namespace::parse(account.to_ascii_lowercase().replace('_', "-"))?;
    let mut scope = ResourceScope::new(namespace);
    for tag in tags_of_settings(settings)? {
        scope = scope.with_tag(tag);
    }
    Ok(scope)
}

/// Intersect an identity/resource policy layer with an explicit permission
/// boundary (HSP-150, AC#1).
///
/// The semantics are an intersection of explicit grants, deliberately simpler
/// than exact AWS IAM/SCP semantics:
///
/// - `Permit` only when **both** layers permit.
/// - A missing grant in either layer is a `Deny` (allow base + no boundary
///   permission = deny; missing base permission = deny even if the boundary
///   permits).
/// - An explicit `forbid` in either layer dominates any permit: the result is
///   `Deny(ExplicitForbid)`.
/// - An evaluation error (`Indeterminate`) is propagated, never converted
///   into `Permit`. When the base layer is indeterminate the boundary is not
///   consulted, because no decision could be reached at all.
pub struct BoundaryAuthorizer {
    base: Arc<dyn AuthorizerPort>,
    boundary: Arc<dyn AuthorizerPort>,
}

impl BoundaryAuthorizer {
    pub fn new(base: Arc<dyn AuthorizerPort>, boundary: Arc<dyn AuthorizerPort>) -> Self {
        Self { base, boundary }
    }
}

impl AuthorizerPort for BoundaryAuthorizer {
    fn authorize(&self, request: &AuthorizationRequest) -> AuthorizationOutcome {
        let base = self.base.authorize(request);
        if matches!(base, AuthorizationOutcome::Indeterminate(_)) {
            return base;
        }
        let boundary = self.boundary.authorize(request);
        if matches!(boundary, AuthorizationOutcome::Indeterminate(_)) {
            return boundary;
        }
        intersect_grants(base, boundary)
    }
}

/// The boundary truth table over two non-indeterminate outcomes.
fn intersect_grants(
    base: AuthorizationOutcome,
    boundary: AuthorizationOutcome,
) -> AuthorizationOutcome {
    use AuthorizationOutcome as Outcome;
    match (&base, &boundary) {
        (Outcome::Permit, Outcome::Permit) => Outcome::Permit,
        (Outcome::Deny(DenyReason::ExplicitForbid), _)
        | (_, Outcome::Deny(DenyReason::ExplicitForbid)) => {
            Outcome::deny(DenyReason::ExplicitForbid)
        }
        (Outcome::Deny(reason), _) | (_, Outcome::Deny(reason)) => Outcome::deny(reason.clone()),
        // The caller short-circuits every Indeterminate and (Permit, Permit)
        // matched first, so no other pair is reachable. Fail closed anyway.
        _ => Outcome::deny(DenyReason::NoMatchingPolicy),
    }
}

/// Memoizes authorization decisions per principal, account, action, resource, policy
/// generation and attribute fingerprint (HSP-150, AC#3).
///
/// Invalidation is a generation counter: [`CachedAuthorizer::invalidate`]
/// (wired to policy bundle changes) makes every cached decision unreachable
/// in one atomic bump. A resource label or attribute change is not a policy
/// change: it alters the fingerprint, so only the decisions computed with the
/// old attributes stop matching. Decisions are never shared across
/// principals, and an `Indeterminate` outcome is returned as such but never
/// cached: a transient evaluator error is re-evaluated (still fail-closed).
pub struct CachedAuthorizer {
    inner: Arc<dyn AuthorizerPort>,
    generation: Arc<AtomicU64>,
    cache: Mutex<HashMap<CacheKey, AuthorizationOutcome>>,
    max_entries: usize,
}

#[derive(PartialEq, Eq, Hash)]
struct CacheKey {
    principal: String,
    account: String,
    groups: String,
    action: String,
    resource: String,
    generation: u64,
    attributes: String,
}

impl CachedAuthorizer {
    /// Build a cache; `max_entries` bounds memory and is a documented
    /// eviction (all entries drop) rather than an LRU, because authorization
    /// caches are correctness-critical and the recomputation cost is an
    /// in-process Cedar evaluation.
    pub fn new(inner: Arc<dyn AuthorizerPort>, max_entries: usize) -> Self {
        Self {
            inner,
            generation: Arc::new(AtomicU64::new(0)),
            cache: Mutex::new(HashMap::new()),
            max_entries: max_entries.max(1),
        }
    }

    /// Record that the policy layer changed: no previously cached decision is
    /// reused, for any principal.
    ///
    /// Ordering contract: swap the policies of the inner evaluator FIRST and
    /// call this AFTER. If the order is reversed, a request that arrives
    /// between the two steps is keyed with the new generation but evaluated
    /// by the old policies, and that stale decision would be cached under the
    /// new generation. Prefer [`CachedAuthorizer::replace_policies`], which
    /// enforces the order.
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.locked_cache().clear();
    }

    /// Run `swap` (which must replace the inner evaluator's policies) and
    /// then invalidate the cache, so the required order cannot be inverted.
    pub fn replace_policies<R>(&self, swap: impl FnOnce() -> R) -> R {
        let result = swap();
        self.invalidate();
        result
    }

    pub fn cached_decisions(&self) -> usize {
        self.locked_cache().len()
    }

    fn locked_cache(&self) -> std::sync::MutexGuard<'_, HashMap<CacheKey, AuthorizationOutcome>> {
        // A panic while holding the lock leaves a well-formed empty or stale
        // map; recovering is fail-closed because stale entries simply miss or
        // are cleared on the next invalidation.
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn key(&self, request: &AuthorizationRequest) -> CacheKey {
        let principal = request.principal();
        let mut attributes = render_attributes(principal.attributes());
        attributes.push('|');
        attributes.push_str(&render_attributes(request.resource().attributes()));
        attributes.push('|');
        attributes.push_str(&render_attributes(request.context_attributes()));
        CacheKey {
            principal: principal.subject().as_str().to_string(),
            account: {
                let mut account = String::new();
                push_field(&mut account, principal.account().as_str());
                account
            },
            groups: principal
                .groups()
                .iter()
                .fold(String::new(), |mut acc, group| {
                    push_field(&mut acc, group.as_str());
                    acc
                }),
            action: format!(
                "{}::{}",
                request.action().service(),
                request.action().name()
            ),
            resource: request.resource().hrn().to_string(),
            generation: self.generation.load(Ordering::SeqCst),
            attributes,
        }
    }
}

/// Append `value` as a length-prefixed field so that no choice of attribute
/// names or values (including `;`, `=`, `+`) can make two different
/// attribute sets render to the same fingerprint.
fn push_field(out: &mut String, value: &str) {
    out.push_str(&value.len().to_string());
    out.push(':');
    out.push_str(value);
}

fn render_attributes(attributes: &Attributes) -> String {
    let mut rendered = String::new();
    for (name, value) in attributes {
        push_field(&mut rendered, name);
        match value {
            AttributeValue::String(value) => {
                rendered.push('s');
                push_field(&mut rendered, value);
            }
            AttributeValue::Bool(value) => rendered.push(if *value { 'T' } else { 'F' }),
            AttributeValue::Long(value) => {
                rendered.push('l');
                push_field(&mut rendered, &value.to_string());
            }
            AttributeValue::Entity(value) => {
                rendered.push('e');
                push_field(&mut rendered, value);
            }
            AttributeValue::Set(values) => {
                rendered.push('S');
                rendered.push_str(&values.len().to_string());
                rendered.push('[');
                for value in values {
                    push_field(&mut rendered, value);
                }
                rendered.push(']');
            }
            // New hodei-authz values still render distinctively through Debug,
            // so a fingerprint change is conservative (recompute), never stale.
            _ => {
                rendered.push('?');
                push_field(&mut rendered, &format!("{value:?}"));
            }
        }
    }
    rendered
}

impl AuthorizerPort for CachedAuthorizer {
    fn authorize(&self, request: &AuthorizationRequest) -> AuthorizationOutcome {
        let key = self.key(request);
        if let Some(cached) = self.locked_cache().get(&key) {
            return cached.clone();
        }
        let outcome = self.inner.authorize(request);
        if matches!(outcome, AuthorizationOutcome::Indeterminate(_)) {
            // Never memoize an evaluation error: a transient failure must not
            // keep blocking the key. Re-evaluating stays fail-closed.
            return outcome;
        }
        let mut cache = self.locked_cache();
        if cache.len() >= self.max_entries {
            cache.clear();
        }
        cache.insert(key, outcome.clone());
        outcome
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
            || principal_hrn.partition() != "hodei"
            || request.resource().hrn().partition() != "hodei"
            || principal_hrn.partition() != request.resource().hrn().partition()
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
        let resource = canonical_resource(hrn)?;
        Ok(self.execute(context, action, resource))
    }

    /// Authorize `action` on a catalog repository with its trusted
    /// [`ResourceScope`] (HSP-321). It is the single entry point that every
    /// repository read, download, publication and listing decision uses, so
    /// a tag-based policy (permit or forbid) sees the same attributes
    /// everywhere. A scope that cannot be built is an `Indeterminate`
    /// outcome (callers already fail closed on it), never a decision made
    /// without the tags.
    pub fn execute_repository(
        &self,
        context: VerifiedContext,
        action: ActionId,
        repository: &hodei_catalog::RepositoryDescriptor,
    ) -> Result<AuthorizationOutcome, hodei_authz::ModelError> {
        match repository_scope(&repository.account, &repository.config.config_settings) {
            Ok(scope) => self.execute_canonical_scoped(context, action, &repository.hrn, scope),
            Err(_) => Ok(AuthorizationOutcome::Indeterminate(
                hodei_authz::IndeterminateReason::EvaluationError,
            )),
        }
    }

    /// [`Self::execute_canonical`] for a resource whose trusted
    /// [`ResourceScope`] (namespace and tags) travels with the request, so a
    /// policy can read `resource.namespace` and `resource.tags` (HSP-321).
    pub fn execute_canonical_scoped(
        &self,
        context: VerifiedContext,
        action: ActionId,
        hrn: &CanonicalHrn,
        scope: ResourceScope,
    ) -> Result<AuthorizationOutcome, hodei_authz::ModelError> {
        let resource = scope.apply(canonical_resource(hrn)?);
        Ok(self.execute(context, action, resource))
    }
}

/// Convert the canonical kernel HRN into a typed resource reference.
fn canonical_resource(hrn: &CanonicalHrn) -> Result<ResourceRef, hodei_authz::ModelError> {
    let authz_hrn = Hrn::new(
        &hrn.partition,
        &hrn.service,
        &hrn.region,
        &hrn.account_id,
        &hrn.resource_type,
        &hrn.resource_id,
    )
    .map_err(|_| hodei_authz::ModelError::InvalidResourceHrn)?;
    ResourceRef::new(authz_hrn)
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

    // -----------------------------------------------------------------
    // HSP-150: ABAC scopes, permission boundary, invalidation.
    // -----------------------------------------------------------------

    use std::sync::atomic::AtomicUsize;

    /// Identity whose claims are fixed, used to verify principals other than
    /// the default `alice`.
    struct ClaimsAuthenticator(PrincipalClaims);

    #[async_trait]
    impl PrincipalAuthenticator for ClaimsAuthenticator {
        async fn authenticate(
            &self,
            _credentials: &Credentials,
        ) -> Result<PrincipalClaims, AuthenticationError> {
            Ok(self.0.clone())
        }
    }

    async fn context_named(subject: &str) -> VerifiedContext {
        VerifiedContext::from_principal(
            verify_principal(
                &ClaimsAuthenticator(PrincipalClaims::new(
                    PrincipalId::new(subject).unwrap(),
                    AccountId::new("acme").unwrap(),
                )),
                &Credentials::bearer("token"),
            )
            .await
            .unwrap(),
        )
    }

    fn repo(account: &str, path: &str) -> ResourceRef {
        let hrn: Hrn = format!("hrn:hodei:artifacts:eu:{account}:repository/{path}")
            .parse()
            .unwrap();
        ResourceRef::new(hrn).unwrap()
    }

    fn scoped(path: &str, namespace: &str, tags: &[&str]) -> ResourceRef {
        let mut scope = ResourceScope::new(Namespace::parse(namespace).unwrap());
        for tag in tags {
            scope = scope.with_tag(ResourceTag::parse(*tag).unwrap());
        }
        scope.apply(repo("acme", path))
    }

    /// Fixed outcome with a call counter, to script the layers of the
    /// boundary and to prove which layers were consulted.
    struct FixedAuthorizer {
        outcome: AuthorizationOutcome,
        calls: Arc<AtomicUsize>,
    }

    impl AuthorizerPort for FixedAuthorizer {
        fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationOutcome {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.outcome.clone()
        }
    }

    /// Delegates to an inner port and records every non-cached call.
    struct RecordingAuthorizer {
        inner: Arc<dyn AuthorizerPort>,
        calls: Arc<AtomicUsize>,
    }

    impl RecordingAuthorizer {
        fn new(inner: Arc<dyn AuthorizerPort>) -> Self {
            Self {
                inner,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl AuthorizerPort for RecordingAuthorizer {
        fn authorize(&self, request: &AuthorizationRequest) -> AuthorizationOutcome {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.authorize(request)
        }
    }

    fn outcome_variants() -> [AuthorizationOutcome; 4] {
        [
            AuthorizationOutcome::Permit,
            AuthorizationOutcome::deny(DenyReason::NoMatchingPolicy),
            AuthorizationOutcome::deny(DenyReason::ExplicitForbid),
            AuthorizationOutcome::indeterminate(IndeterminateReason::EvaluationError),
        ]
    }

    /// The full 4x4 truth table of base x boundary through the composed
    /// authorizer (the exhaustive parametrized form required by AC#4;
    /// proptest is not in the workspace dev-deps).
    #[tokio::test]
    async fn boundary_truth_table_is_exhaustive() {
        use AuthorizationOutcome as Outcome;
        // Expected intersection: indeterminate propagates (base first,
        // boundary short-circuited); Permit only when both permit; an
        // explicit forbid dominates; any other deny keeps its reason.
        let expected = |base: &Outcome, boundary: &Outcome| -> Outcome {
            match (base, boundary) {
                (Outcome::Indeterminate(reason), _) => Outcome::Indeterminate(reason.clone()),
                (_, Outcome::Indeterminate(reason)) => Outcome::Indeterminate(reason.clone()),
                (Outcome::Permit, Outcome::Permit) => Outcome::Permit,
                (Outcome::Deny(DenyReason::ExplicitForbid), _)
                | (_, Outcome::Deny(DenyReason::ExplicitForbid)) => {
                    Outcome::deny(DenyReason::ExplicitForbid)
                }
                (Outcome::Deny(reason), _) | (_, Outcome::Deny(reason)) => {
                    Outcome::deny(reason.clone())
                }
            }
        };

        let request = AuthorizationRequest::new(context().await, action(), resource("acme"));
        for base in outcome_variants() {
            for boundary in outcome_variants() {
                let boundary_calls = Arc::new(AtomicUsize::new(0));
                let composed = BoundaryAuthorizer::new(
                    Arc::new(FixedAuthorizer {
                        outcome: base.clone(),
                        calls: Arc::new(AtomicUsize::new(0)),
                    }),
                    Arc::new(FixedAuthorizer {
                        outcome: boundary.clone(),
                        calls: boundary_calls.clone(),
                    }),
                );
                let got = composed.authorize(&request);
                let want = expected(&base, &boundary);
                assert_eq!(got, want, "base={base:?} boundary={boundary:?}");
                if matches!(base, Outcome::Indeterminate(_)) {
                    assert_eq!(
                        boundary_calls.load(Ordering::SeqCst),
                        0,
                        "an indeterminate base must not consult the boundary"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn allow_without_boundary_permission_is_denied() {
        // AC#1: allow in the base, no grant in the boundary => deny.
        let base = cedar(
            r#"permit(
                principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
                action == Artifacts::Action::"ReadRepository",
                resource
            );"#,
        );
        let boundary = Arc::new(DenyAllAuthorizer);
        let use_case =
            AuthorizeResourceUseCase::new(Arc::new(BoundaryAuthorizer::new(base, boundary)));
        assert_eq!(
            use_case.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
        );
    }

    #[tokio::test]
    async fn boundary_permission_without_base_is_denied() {
        // AC#1: the boundary alone never grants; the base must permit too.
        let base = Arc::new(DenyAllAuthorizer);
        let boundary = cedar(
            r#"permit(
                principal, action, resource
            );"#,
        );
        let use_case =
            AuthorizeResourceUseCase::new(Arc::new(BoundaryAuthorizer::new(base, boundary)));
        assert_eq!(
            use_case.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
        );
    }

    #[tokio::test]
    async fn forbid_in_either_layer_dominates_a_permit() {
        // AC#1: explicit forbid wins regardless of the other layer.
        let permitting = cedar(
            r#"permit(
                principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
                action == Artifacts::Action::"ReadRepository",
                resource
            );"#,
        );
        let forbidding = cedar(
            r#"forbid(
                principal, action, resource
            );"#,
        );
        let base_forbids = AuthorizeResourceUseCase::new(Arc::new(BoundaryAuthorizer::new(
            forbidding.clone(),
            permitting.clone(),
        )));
        assert_eq!(
            base_forbids.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Deny(DenyReason::ExplicitForbid)
        );
        let boundary_forbids = AuthorizeResourceUseCase::new(Arc::new(BoundaryAuthorizer::new(
            permitting, forbidding,
        )));
        assert_eq!(
            boundary_forbids.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Deny(DenyReason::ExplicitForbid)
        );
    }

    #[tokio::test]
    async fn engine_error_becomes_indeterminate_and_blocks() {
        // AC#3/AC#1: a broken base is never silently allowed by a healthy
        // boundary.
        let broken_base = cedar(
            r#"permit(
                principal, action, resource
            ) when { principal.role == "admin" };"#,
        );
        let healthy_boundary = cedar(
            r#"permit(
                principal, action, resource
            );"#,
        );
        let use_case = AuthorizeResourceUseCase::new(Arc::new(BoundaryAuthorizer::new(
            broken_base,
            healthy_boundary,
        )));
        assert_eq!(
            use_case.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Indeterminate(IndeterminateReason::EvaluationError)
        );
    }

    #[tokio::test]
    async fn abac_namespace_and_tags_are_evaluated_by_cedar() {
        // AC#2: trusted namespace and tags drive the base policy.
        let base = cedar(
            r#"permit(
                principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
                action == Artifacts::Action::"ReadRepository",
                resource
            ) when {
                resource.namespace == "com.example" &&
                resource.tags.contains("release")
            };"#,
        );
        let boundary = cedar(
            r#"permit(
                principal, action, resource
            ) when {
                resource in Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/releases"
            };
            forbid(
                principal, action, resource
            ) when {
                !(resource in Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/releases")
            };"#,
        );
        let use_case =
            AuthorizeResourceUseCase::new(Arc::new(BoundaryAuthorizer::new(base, boundary)));

        // Right namespace, right tag, inside the boundary prefix: allowed.
        assert_eq!(
            use_case.execute(
                context().await,
                action(),
                scoped("releases/v2", "com.example", &["release", "stable"]),
            ),
            AuthorizationOutcome::Permit
        );
        // Same repository path but a different namespace: the ABAC policy
        // denies; access is not cross-namespace.
        assert_eq!(
            use_case.execute(
                context().await,
                action(),
                scoped("releases/v2", "org.other", &["release"]),
            ),
            AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
        );
        // Right namespace but missing the required tag.
        assert_eq!(
            use_case.execute(
                context().await,
                action(),
                scoped("releases/v2", "com.example", &["draft"]),
            ),
            AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
        );
        // Outside the boundary path prefix: the explicit forbid dominates,
        // even with the right labels. The boundary wildcard (parent scope)
        // does not leak to `staging`.
        assert_eq!(
            use_case.execute(
                context().await,
                action(),
                scoped("staging/v2", "com.example", &["release"]),
            ),
            AuthorizationOutcome::Deny(DenyReason::ExplicitForbid)
        );
        // A resource without trusted attributes cannot satisfy the `when`
        // clause; Cedar errors and the outcome is indeterminate, never
        // permit.
        assert_eq!(
            use_case.execute(context().await, action(), repo("acme", "releases/v2")),
            AuthorizationOutcome::Indeterminate(IndeterminateReason::EvaluationError)
        );
    }

    #[tokio::test]
    async fn scope_attributes_are_the_only_trusted_resource_attributes() {
        // The typed scope exports exactly namespace and tags.
        let scope = ResourceScope::new(Namespace::parse("com.example").unwrap())
            .with_tag(ResourceTag::parse("release").unwrap());
        let attributes = scope.trusted_attributes();
        assert_eq!(attributes.len(), 2);
        assert_eq!(
            attributes.get("namespace"),
            Some(&AttributeValue::String("com.example".to_string()))
        );
        assert_eq!(
            attributes.get("tags"),
            Some(&AttributeValue::Set(
                ["release".to_string()].into_iter().collect()
            ))
        );
    }

    /// A scripted policy layer: permits exactly one subject suffix, counts
    /// engine calls, and its grant can be rotated to simulate a policy
    /// bundle change behind the same port.
    struct GrantSwitcher {
        allow_suffix: Mutex<String>,
        calls: Arc<AtomicUsize>,
    }

    impl GrantSwitcher {
        fn new(allow_suffix: &str) -> Self {
            Self {
                allow_suffix: Mutex::new(allow_suffix.to_string()),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn grant_to(&self, allow_suffix: &str) {
            *self
                .allow_suffix
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = allow_suffix.to_string();
        }
    }

    impl AuthorizerPort for GrantSwitcher {
        fn authorize(&self, request: &AuthorizationRequest) -> AuthorizationOutcome {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let allowed = self
                .allow_suffix
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if request.principal().subject().as_str().ends_with(&allowed) {
                AuthorizationOutcome::Permit
            } else {
                AuthorizationOutcome::deny(DenyReason::NoMatchingPolicy)
            }
        }
    }

    #[tokio::test]
    async fn policy_change_invalidates_every_principal_and_is_not_reused() {
        // AC#3: after a policy change plus invalidation, no principal reuses
        // an old decision, and decisions never cross principals.
        let policy = Arc::new(GrantSwitcher::new("alice"));
        let calls = policy.calls.clone();
        let cached = Arc::new(CachedAuthorizer::new(policy.clone(), 64));
        let use_case = AuthorizeResourceUseCase::new(cached.clone());

        // Alice is permitted and cached with one engine call.
        assert_eq!(
            use_case.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Permit
        );
        assert_eq!(
            use_case.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Permit
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(cached.cached_decisions(), 1);

        // Bob must never inherit Alice's cached permit: his request is a
        // cache miss and the engine denies it.
        assert_eq!(
            use_case.execute(
                context_named("hrn:hodei:iam::acme:User/bob").await,
                action(),
                resource("acme"),
            ),
            AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // The policy bundle changes (alice loses her grant) and the
        // generation is invalidated: her next request re-evaluates and is
        // denied. The stale permit cannot be reused.
        policy.grant_to("carol");
        cached.invalidate();
        assert_eq!(cached.cached_decisions(), 0);
        assert_eq!(
            use_case.execute(context().await, action(), resource("acme")),
            AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        // And carol, the new grantee, is permitted on her first request.
        assert_eq!(
            use_case.execute(
                context_named("hrn:hodei:iam::acme:User/carol").await,
                action(),
                resource("acme"),
            ),
            AuthorizationOutcome::Permit
        );
    }

    #[tokio::test]
    async fn label_change_recomputes_and_never_reuses_the_old_decision() {
        // AC#3: changing resource labels alters the fingerprint, so the
        // previous decision for the same principal and resource is not
        // returned.
        let inner = cedar(
            r#"permit(
                principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
                action == Artifacts::Action::"ReadRepository",
                resource
            ) when { resource.tags.contains("release") };"#,
        );
        let recorder = Arc::new(RecordingAuthorizer::new(inner));
        let cached = Arc::new(CachedAuthorizer::new(recorder.clone(), 64));
        let use_case = AuthorizeResourceUseCase::new(cached);

        assert_eq!(
            use_case.execute(
                context().await,
                action(),
                scoped("releases/v2", "com.example", &["release"]),
            ),
            AuthorizationOutcome::Permit
        );
        assert_eq!(
            use_case.execute(
                context().await,
                action(),
                scoped("releases/v2", "com.example", &[]),
            ),
            AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
        );
        assert_eq!(recorder.calls.load(Ordering::SeqCst), 2);

        // Repeating the new labels hits the cache for the new fingerprint,
        // not the old permit.
        assert_eq!(
            use_case.execute(
                context().await,
                action(),
                scoped("releases/v2", "com.example", &[]),
            ),
            AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
        );
        assert_eq!(recorder.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn indeterminate_is_never_cached_and_stays_blocking() {
        // AC#3: an engine error blocks every time and is re-evaluated, so a
        // transient failure cannot pin a key.
        let inner = cedar(
            r#"permit(
                principal, action, resource
            ) when { principal.role == "admin" };"#,
        );
        let recorder = Arc::new(RecordingAuthorizer::new(inner));
        let cached = Arc::new(CachedAuthorizer::new(recorder.clone(), 8));
        let use_case = AuthorizeResourceUseCase::new(cached.clone());

        for _ in 0..3 {
            assert_eq!(
                use_case.execute(context().await, action(), resource("acme")),
                AuthorizationOutcome::Indeterminate(IndeterminateReason::EvaluationError)
            );
        }
        assert_eq!(recorder.calls.load(Ordering::SeqCst), 3);
        assert_eq!(cached.cached_decisions(), 0);
    }

    #[tokio::test]
    async fn permit_and_deny_are_still_cached() {
        let policy = Arc::new(GrantSwitcher::new("alice"));
        let calls = policy.calls.clone();
        let cached = Arc::new(CachedAuthorizer::new(policy, 8));
        let use_case = AuthorizeResourceUseCase::new(cached.clone());
        for _ in 0..3 {
            assert_eq!(
                use_case.execute(
                    context_named("hrn:hodei:iam::acme:User/alice").await,
                    action(),
                    resource("acme")
                ),
                AuthorizationOutcome::Permit
            );
            assert!(matches!(
                use_case.execute(
                    context_named("hrn:hodei:iam::acme:User/bob").await,
                    action(),
                    resource("acme")
                ),
                AuthorizationOutcome::Deny(_)
            ));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(cached.cached_decisions(), 2);
    }

    #[tokio::test]
    async fn same_subject_in_another_account_does_not_share_an_entry() {
        let policy = Arc::new(GrantSwitcher::new("alice"));
        let calls = policy.calls.clone();
        let cached = CachedAuthorizer::new(policy, 8);
        let mut requests = Vec::new();
        for account in ["acme", "beta", "acme"] {
            let context = VerifiedContext::from_principal(
                verify_principal(
                    &ClaimsAuthenticator(PrincipalClaims::new(
                        PrincipalId::new("alice").unwrap(),
                        AccountId::new(account).unwrap(),
                    )),
                    &Credentials::bearer("token"),
                )
                .await
                .unwrap(),
            );
            requests.push(AuthorizationRequest::new(
                context,
                action(),
                resource("acme"),
            ));
        }
        let _ = cached.authorize(&requests[0]);
        let _ = cached.authorize(&requests[1]);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let _ = cached.authorize(&requests[2]);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn replace_policies_swaps_then_invalidates() {
        let policy = Arc::new(GrantSwitcher::new("alice"));
        let cached = CachedAuthorizer::new(policy.clone(), 8);
        let use_case_cache = Arc::new(cached);
        let use_case = AuthorizeResourceUseCase::new(use_case_cache.clone());
        assert_eq!(
            use_case.execute(
                context_named("hrn:hodei:iam::acme:User/alice").await,
                action(),
                resource("acme")
            ),
            AuthorizationOutcome::Permit
        );
        use_case_cache.replace_policies(|| policy.grant_to("bob"));
        assert!(matches!(
            use_case.execute(
                context_named("hrn:hodei:iam::acme:User/alice").await,
                action(),
                resource("acme")
            ),
            AuthorizationOutcome::Deny(_)
        ));
    }

    #[test]
    fn namespace_validation_rejects_untrusted_shapes() {
        assert_eq!(
            Namespace::parse("com.example").unwrap().as_str(),
            "com.example"
        );
        assert_eq!(Namespace::parse("crates-io").unwrap().as_str(), "crates-io");
        assert!(Namespace::parse("").is_err());
        assert!(Namespace::parse("Com.Example").is_err());
        assert!(Namespace::parse("com..example").is_err());
        assert!(Namespace::parse("com.example_").is_err());
        assert!(Namespace::parse(format!("a{}", "b".repeat(63))).is_err());
        let long_tail = std::iter::repeat_n("segment.", 40).collect::<String>();
        assert!(Namespace::parse(format!("{long_tail}end")).is_err());
    }

    #[test]
    fn tag_validation_rejects_untrusted_shapes() {
        assert_eq!(
            ResourceTag::parse("release-2026").unwrap().as_str(),
            "release-2026"
        );
        assert!(ResourceTag::parse("").is_err());
        assert!(ResourceTag::parse("Release").is_err());
        assert!(ResourceTag::parse("has space").is_err());
        assert!(ResourceTag::parse("has\"quote").is_err());
        assert!(ResourceTag::parse("t".repeat(129)).is_err());
    }

    #[test]
    fn rendered_attribute_fingerprint_is_order_stable() {
        // The cache fingerprint sorts attributes (BTreeMap) and set members
        // (BTreeSet), so equal scopes always collide and different scopes
        // never do.
        let mut first = Attributes::new();
        first.insert(
            "tags".to_string(),
            AttributeValue::Set(["a".to_string(), "b".to_string()].into_iter().collect()),
        );
        first.insert(
            "namespace".to_string(),
            AttributeValue::String("com.example".into()),
        );
        let mut second = Attributes::new();
        second.insert(
            "namespace".to_string(),
            AttributeValue::String("com.example".into()),
        );
        second.insert(
            "tags".to_string(),
            AttributeValue::Set(["b".to_string(), "a".to_string()].into_iter().collect()),
        );
        assert_eq!(render_attributes(&first), render_attributes(&second));

        let mut third = first.clone();
        third.insert(
            "namespace".to_string(),
            AttributeValue::String("org.other".into()),
        );
        assert_ne!(render_attributes(&first), render_attributes(&third));
    }

    #[test]
    fn attribute_fingerprint_is_injective_over_delimiter_characters() {
        // One attribute whose value smuggles the separators of a second one
        // must not collide with two genuine attributes.
        let mut smuggled = Attributes::new();
        smuggled.insert("a".to_string(), AttributeValue::String("x;b=y".into()));
        let mut genuine = Attributes::new();
        genuine.insert("a".to_string(), AttributeValue::String("x".into()));
        genuine.insert("b".to_string(), AttributeValue::String("y".into()));
        assert_ne!(render_attributes(&smuggled), render_attributes(&genuine));

        let mut joined = Attributes::new();
        joined.insert(
            "t".to_string(),
            AttributeValue::Set(["a+b".to_string()].into_iter().collect()),
        );
        let mut split = Attributes::new();
        split.insert(
            "t".to_string(),
            AttributeValue::Set(["a".to_string(), "b".to_string()].into_iter().collect()),
        );
        assert_ne!(render_attributes(&joined), render_attributes(&split));
    }

    #[test]
    fn repository_scope_reads_validated_tags_and_fails_closed() {
        let scope = repository_scope("Acme_Co", &serde_json::json!({"tags": ["prod", "eu"]}))
            .expect("a scope");
        assert_eq!(scope.namespace().as_str(), "acme-co");
        assert_eq!(scope.tags().len(), 2);
        assert!(
            repository_scope("acme", &serde_json::json!({}))
                .unwrap()
                .tags()
                .is_empty()
        );
        for bad in [
            serde_json::json!({"tags": "prod"}),
            serde_json::json!({"tags": [1]}),
            serde_json::json!({"tags": ["NOT OK"]}),
        ] {
            assert!(repository_scope("acme", &bad).is_err(), "{bad}");
        }
        assert!(repository_scope("acme-", &serde_json::json!({})).is_err());
    }
}
