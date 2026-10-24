//! Infrastructure adapters for evaluate_iam_policies feature
//!
//! This module provides concrete implementations of the ports the IAM
//! evaluation depends on:
//!
//! - [`InMemoryPolicyFinderAdapter`]: a policy finder that selects the
//!   policies attached to the principal being decided about.
//! - [`HrnEntityResolver`]: a resolver that materializes the principal and the
//!   resource entities from their HRNs.
//!
//! The persistence-backed policy finder lives with the rest of the SurrealDB
//! adapters (see `crate::infrastructure::surreal::policy_adapter`), because the
//! database handle lives there.

use kernel::Hrn;
use kernel::domain::policy::{HodeiPolicy, HodeiPolicySet};
use kernel::{AttributeName, AttributeValue, HodeiEntity};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

use super::ports::{
    EntityResolverError, PolicyFinderError, PolicyFinderPort, PrincipalResolverPort,
    ResourceResolverPort,
};
use async_trait::async_trait;

/// A policy together with the principals it is attached to.
///
/// The attachment is what makes the policy *effective* for a principal: the
/// evaluation consults the policies attached to the principal it is deciding
/// about, and nothing else.
#[derive(Debug, Clone)]
pub struct AttachedPolicy {
    /// The policy itself.
    pub policy: HodeiPolicy,

    /// The canonical HRNs of the principals the policy is attached to.
    pub attached_principals: Vec<String>,
}

impl AttachedPolicy {
    /// Build an attached policy.
    pub fn new(policy: HodeiPolicy, attached_principals: Vec<String>) -> Self {
        Self {
            policy,
            attached_principals,
        }
    }

    /// Whether the policy is attached to the given principal.
    ///
    /// The comparison is by canonical HRN string, which is the same form the
    /// persistence layer stores and the same form the query binds.
    pub fn is_attached_to(&self, principal_hrn: &Hrn) -> bool {
        let principal = principal_hrn.to_string();
        self.attached_principals.iter().any(|hrn| hrn == &principal)
    }
}

/// In-memory implementation of `PolicyFinderPort`.
///
/// The policies are held with their attachment, and the finder selects the
/// ones attached to the principal being decided about. Returning *every*
/// policy instead would make each policy a global permit: a policy written
/// for one principal would grant to every other principal, which is the
/// opposite of what the attachment means.
#[derive(Debug, Clone)]
pub struct InMemoryPolicyFinderAdapter {
    policies: Arc<Vec<AttachedPolicy>>,
}

impl InMemoryPolicyFinderAdapter {
    /// Create a new in-memory policy finder with no policies
    pub fn new() -> Self {
        Self {
            policies: Arc::new(Vec::new()),
        }
    }

    /// Create a new in-memory policy finder with initial attached policies
    pub fn with_policies(policies: Vec<AttachedPolicy>) -> Self {
        Self {
            policies: Arc::new(policies),
        }
    }
}

impl Default for InMemoryPolicyFinderAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolicyFinderPort for InMemoryPolicyFinderAdapter {
    async fn get_effective_policies(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<HodeiPolicySet, PolicyFinderError> {
        debug!("Finding policies for principal: {}", principal_hrn);

        let effective: Vec<HodeiPolicy> = self
            .policies
            .iter()
            .filter(|attached| attached.is_attached_to(principal_hrn))
            .map(|attached| attached.policy.clone())
            .collect();

        let policy_set = HodeiPolicySet::new(effective);

        info!(
            "Found {} policies for principal: {}",
            policy_set.len(),
            principal_hrn
        );
        Ok(policy_set)
    }
}

/// An entity that carries nothing but its HRN.
///
/// The Cedar entity uid is derived from the HRN, so an entity that carries
/// only its HRN is still translatable and still identifies the same
/// principal or resource.
#[derive(Debug, Clone)]
pub struct HrnOnlyEntity {
    hrn: Hrn,
}

impl HrnOnlyEntity {
    /// Build an entity that carries only its HRN.
    pub fn new(hrn: Hrn) -> Self {
        Self { hrn }
    }
}

impl HodeiEntity for HrnOnlyEntity {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
        HashMap::new()
    }
}

/// Resolver that materializes entities from their HRNs.
///
/// # Degradation, declared
///
/// The platform has no identity profile yet — there is no API that creates a
/// principal, and the IAM user routes are not mounted — so the only thing
/// known about a principal is its HRN. The resolver therefore returns an
/// entity with **no attributes**.
///
/// The consequence is precise and bounded: a policy conditioned on an
/// attribute of the principal or of the resource cannot match, because the
/// attribute is not there. A policy that constrains the principal or the
/// resource *by identity* — which is the form the attachment makes
/// meaningful — evaluates correctly.
#[derive(Debug, Clone, Default)]
pub struct HrnEntityResolver;

impl HrnEntityResolver {
    /// Create a new resolver.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PrincipalResolverPort for HrnEntityResolver {
    async fn resolve_principal(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<Box<dyn HodeiEntity + Send>, EntityResolverError> {
        debug!("Resolving principal from its HRN: {}", principal_hrn);
        Ok(Box::new(HrnOnlyEntity::new(principal_hrn.clone())))
    }
}

#[async_trait]
impl ResourceResolverPort for HrnEntityResolver {
    async fn resolve_resource(
        &self,
        resource_hrn: &Hrn,
    ) -> Result<Box<dyn HodeiEntity + Send>, EntityResolverError> {
        debug!("Resolving resource from its HRN: {}", resource_hrn);
        Ok(Box::new(HrnOnlyEntity::new(resource_hrn.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::domain::policy::PolicyId;

    fn attached(policy_id: &str, principals: &[&str]) -> AttachedPolicy {
        AttachedPolicy::new(
            HodeiPolicy::new(
                PolicyId::new(policy_id.to_string()),
                "permit(principal, action, resource);".to_string(),
            ),
            principals.iter().map(|p| p.to_string()).collect(),
        )
    }

    #[tokio::test]
    async fn test_in_memory_policy_finder_empty() {
        let finder = InMemoryPolicyFinderAdapter::new();
        let principal_hrn = Hrn::from_string("hrn:aws:iam::account-a:User/alice").unwrap();

        let result = finder.get_effective_policies(&principal_hrn).await;
        assert!(result.is_ok());

        let policy_set = result.unwrap();
        assert_eq!(policy_set.policies().len(), 0);
    }

    /// A policy attached to somebody else is not effective for this principal.
    #[tokio::test]
    async fn test_in_memory_policy_finder_only_returns_attached_policies() {
        let finder = InMemoryPolicyFinderAdapter::with_policies(vec![attached(
            "alice-policy",
            &["hrn:aws:iam::account-a:User/alice"],
        )]);
        let principal_hrn = Hrn::from_string("hrn:aws:iam::account-a:User/alice").unwrap();

        let policy_set = finder
            .get_effective_policies(&principal_hrn)
            .await
            .expect("the finder answers");

        assert_eq!(policy_set.policies().len(), 1);
        assert_eq!(policy_set.policies()[0].id().to_string(), "alice-policy");
    }

    /// The same policy, evaluated for a principal it is not attached to, is
    /// not consulted at all.
    #[tokio::test]
    async fn test_in_memory_policy_finder_does_not_leak_to_other_principals() {
        let finder = InMemoryPolicyFinderAdapter::with_policies(vec![attached(
            "alice-policy",
            &["hrn:aws:iam::account-a:User/alice"],
        )]);
        let other_hrn = Hrn::from_string("hrn:aws:iam::account-b:User/bob").unwrap();

        let policy_set = finder
            .get_effective_policies(&other_hrn)
            .await
            .expect("the finder answers");

        assert!(
            policy_set.policies().is_empty(),
            "a policy attached to alice must not be effective for bob"
        );
    }

    /// The resolver returns an entity that identifies the same principal.
    #[tokio::test]
    async fn test_hrn_entity_resolver_materializes_the_hrn() {
        let resolver = HrnEntityResolver::new();
        let principal_hrn = Hrn::from_string("hrn:aws:iam::account-a:User/alice").unwrap();

        let entity = resolver
            .resolve_principal(&principal_hrn)
            .await
            .expect("the resolver answers");

        assert_eq!(entity.hrn(), &principal_hrn);
        assert!(
            entity.attributes().is_empty(),
            "the degradation is declared: no attributes without an identity profile"
        );
    }

    #[tokio::test]
    async fn test_hrn_entity_resolver_materializes_the_resource() {
        let resolver = HrnEntityResolver::new();
        let resource_hrn =
            Hrn::from_string("hrn:hodei:artifacts::acme:Repository/releases").unwrap();

        let entity = resolver
            .resolve_resource(&resource_hrn)
            .await
            .expect("the resolver answers");

        assert_eq!(entity.hrn(), &resource_hrn);
    }
}
