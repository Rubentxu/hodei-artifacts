use std::sync::Arc;

use crate::features::evaluate_permissions::dto::AuthorizationResponse;
use crate::features::evaluate_permissions::error::EvaluatePermissionsResult;
use crate::features::evaluate_permissions::ports::{
    AuthorizationCache, AuthorizationLogger, AuthorizationMetrics, EntityResolver,
    IamPolicyProvider, OrganizationBoundaryProvider,
};
use crate::features::evaluate_permissions::use_case::EvaluatePermissionsUseCase;
use async_trait::async_trait;
use policies::shared::domain::hrn::Hrn;

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
#[derive(Debug)]
pub struct EvaluatePermissionsContainer<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER> {
    iam_provider: IAM,
    org_provider: ORG,
    cache: Option<CACHE>,
    logger: LOGGER,
    metrics: METRICS,
    entity_resolver: RESOLVER,
}

impl<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER>
    EvaluatePermissionsContainer<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER>
where
    IAM: IamPolicyProvider,
    ORG: OrganizationBoundaryProvider,
    CACHE: AuthorizationCache,
    LOGGER: AuthorizationLogger,
    METRICS: AuthorizationMetrics,
    RESOLVER: EntityResolver,
{
    /// Create a new dependency injection container
    pub fn new(
        iam_provider: IAM,
        org_provider: ORG,
        cache: Option<CACHE>,
        logger: LOGGER,
        metrics: METRICS,
        entity_resolver: RESOLVER,
    ) -> Self {
        Self {
            iam_provider,
            org_provider,
            cache,
            logger,
            metrics,
            entity_resolver,
        }
    }

    /// Build the EvaluatePermissionsUseCase with all dependencies injected
    pub fn build_use_case(
        self,
    ) -> EvaluatePermissionsUseCase<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER> {
        EvaluatePermissionsUseCase::new(
            self.iam_provider,
            self.org_provider,
            self.cache,
            self.logger,
            self.metrics,
            self.entity_resolver,
        )
    }
}

/// Builder pattern for creating the dependency injection container
pub struct EvaluatePermissionsContainerBuilder<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER> {
    iam_provider: Option<IAM>,
    org_provider: Option<ORG>,
    cache: Option<CACHE>,
    logger: Option<LOGGER>,
    metrics: Option<METRICS>,
    entity_resolver: Option<RESOLVER>,
}

impl<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER>
    EvaluatePermissionsContainerBuilder<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER>
where
    IAM: IamPolicyProvider,
    ORG: OrganizationBoundaryProvider,
    CACHE: AuthorizationCache,
    LOGGER: AuthorizationLogger,
    METRICS: AuthorizationMetrics,
    RESOLVER: EntityResolver,
{
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            iam_provider: None,
            org_provider: None,
            cache: None,
            logger: None,
            metrics: None,
            entity_resolver: None,
        }
    }

    /// Set the IAM policy provider
    pub fn with_iam_provider(mut self, iam_provider: IAM) -> Self {
        self.iam_provider = Some(iam_provider);
        self
    }

    /// Set the organization boundary provider
    pub fn with_org_provider(mut self, org_provider: ORG) -> Self {
        self.org_provider = Some(org_provider);
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

    /// Set the entity resolver
    pub fn with_entity_resolver(mut self, entity_resolver: RESOLVER) -> Self {
        self.entity_resolver = Some(entity_resolver);
        self
    }

    /// Build the container
    pub fn build(
        self,
    ) -> Result<EvaluatePermissionsContainer<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER>, String>
    {
        Ok(EvaluatePermissionsContainer::new(
            self.iam_provider.ok_or("IAM provider is required")?,
            self.org_provider
                .ok_or("Organization provider is required")?,
            self.cache,
            self.logger.ok_or("Logger is required")?,
            self.metrics.ok_or("Metrics is required")?,
            self.entity_resolver.ok_or("Entity resolver is required")?,
        ))
    }
}

impl<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER> Default
    for EvaluatePermissionsContainerBuilder<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER>
where
    IAM: IamPolicyProvider,
    ORG: OrganizationBoundaryProvider,
    CACHE: AuthorizationCache,
    LOGGER: AuthorizationLogger,
    METRICS: AuthorizationMetrics,
    RESOLVER: EntityResolver,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Factory functions for common dependency configurations
pub mod factories {
    use super::*;

    /// Create a container with all required dependencies (no cache)
    pub fn create_without_cache<IAM, ORG, LOGGER, METRICS, RESOLVER>(
        iam_provider: IAM,
        org_provider: ORG,
        logger: LOGGER,
        metrics: METRICS,
        entity_resolver: RESOLVER,
    ) -> EvaluatePermissionsContainer<IAM, ORG, DummyCache, LOGGER, METRICS, RESOLVER>
    where
        IAM: IamPolicyProvider,
        ORG: OrganizationBoundaryProvider,
        LOGGER: AuthorizationLogger,
        METRICS: AuthorizationMetrics,
        RESOLVER: EntityResolver,
    {
        EvaluatePermissionsContainer::new(
            iam_provider,
            org_provider,
            Some(DummyCache),
            logger,
            metrics,
            entity_resolver,
        )
    }

    /// Create a container with cache enabled
    pub fn create_with_cache<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER>(
        iam_provider: IAM,
        org_provider: ORG,
        cache: CACHE,
        logger: LOGGER,
        metrics: METRICS,
        entity_resolver: RESOLVER,
    ) -> EvaluatePermissionsContainer<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER>
    where
        IAM: IamPolicyProvider,
        ORG: OrganizationBoundaryProvider,
        CACHE: AuthorizationCache,
        LOGGER: AuthorizationLogger,
        METRICS: AuthorizationMetrics,
        RESOLVER: EntityResolver,
    {
        EvaluatePermissionsContainer::new(
            iam_provider,
            org_provider,
            Some(cache),
            logger,
            metrics,
            entity_resolver,
        )
    }

    /// Create a container from Arc-wrapped dependencies (useful for shared services)
    pub fn create_from_arcs<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER>(
        iam_provider: Arc<IAM>,
        org_provider: Arc<ORG>,
        cache: Option<CACHE>,
        logger: Arc<LOGGER>,
        metrics: Arc<METRICS>,
        entity_resolver: Arc<RESOLVER>,
    ) -> EvaluatePermissionsContainer<
        Arc<IAM>,
        Arc<ORG>,
        CACHE,
        Arc<LOGGER>,
        Arc<METRICS>,
        Arc<RESOLVER>,
    >
    where
        IAM: IamPolicyProvider + 'static,
        ORG: OrganizationBoundaryProvider + 'static,
        CACHE: AuthorizationCache + 'static,
        LOGGER: AuthorizationLogger + 'static,
        METRICS: AuthorizationMetrics + 'static,
        RESOLVER: EntityResolver + 'static,
    {
        EvaluatePermissionsContainer::new(
            iam_provider,
            org_provider,
            cache,
            logger,
            metrics,
            entity_resolver,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::evaluate_permissions::mocks::{
        MockAuthorizationCache, MockAuthorizationLogger, MockAuthorizationMetrics,
        MockEntityResolver, MockIamPolicyProvider, MockOrganizationBoundaryProvider,
    };

    #[test]
    fn test_builder_pattern() {
        let iam_provider = MockIamPolicyProvider::new();
        let org_provider = MockOrganizationBoundaryProvider::new();
        let cache = MockAuthorizationCache::new();
        let logger = MockAuthorizationLogger::new();
        let metrics = MockAuthorizationMetrics::new();
        let entity_resolver = MockEntityResolver::new();

        let container = EvaluatePermissionsContainerBuilder::new()
            .with_iam_provider(iam_provider)
            .with_org_provider(org_provider)
            .with_cache(cache)
            .with_logger(logger)
            .with_metrics(metrics)
            .with_entity_resolver(entity_resolver)
            .build();

        assert!(container.is_ok());
    }

    #[test]
    fn test_builder_missing_required_dependency() {
        let result: Result<
            EvaluatePermissionsContainer<
                MockIamPolicyProvider,
                MockOrganizationBoundaryProvider,
                MockAuthorizationCache,
                MockAuthorizationLogger,
                MockAuthorizationMetrics,
                MockEntityResolver,
            >,
            String,
        > = EvaluatePermissionsContainerBuilder::new()
            .with_org_provider(MockOrganizationBoundaryProvider::new())
            .with_logger(MockAuthorizationLogger::new())
            .with_metrics(MockAuthorizationMetrics::new())
            .with_entity_resolver(MockEntityResolver::new())
            .build();

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("IAM provider is required"));
    }

    #[test]
    fn test_factory_without_cache() {
        let iam_provider = MockIamPolicyProvider::new();
        let org_provider = MockOrganizationBoundaryProvider::new();
        let logger = MockAuthorizationLogger::new();
        let metrics = MockAuthorizationMetrics::new();
        let entity_resolver = MockEntityResolver::new();

        let container = factories::create_without_cache(
            iam_provider,
            org_provider,
            logger,
            metrics,
            entity_resolver,
        );

        let use_case = container.build_use_case();
        // The use case should be created successfully
        assert!(true); // If we get here, construction succeeded
    }

    #[test]
    fn test_factory_with_cache() {
        let iam_provider = MockIamPolicyProvider::new();
        let org_provider = MockOrganizationBoundaryProvider::new();
        let cache = MockAuthorizationCache::new();
        let logger = MockAuthorizationLogger::new();
        let metrics = MockAuthorizationMetrics::new();
        let entity_resolver = MockEntityResolver::new();

        let container = factories::create_with_cache(
            iam_provider,
            org_provider,
            cache,
            logger,
            metrics,
            entity_resolver,
        );

        let use_case = container.build_use_case();
        // The use case should be created successfully
        assert!(true); // If we get here, construction succeeded
    }

    #[test]
    fn test_factory_from_arcs() {
        let iam_provider = Arc::new(MockIamPolicyProvider::new());
        let org_provider = Arc::new(MockOrganizationBoundaryProvider::new());
        let cache = Arc::new(MockAuthorizationCache::new());
        let logger = Arc::new(MockAuthorizationLogger::new());
        let metrics = Arc::new(MockAuthorizationMetrics::new());
        let entity_resolver = Arc::new(MockEntityResolver::new());

        let container = factories::create_from_arcs(
            iam_provider,
            org_provider,
            Some(cache),
            logger,
            metrics,
            entity_resolver,
        );

        let use_case = container.build_use_case();
        // The use case should be created successfully
        assert!(true); // If we get here, construction succeeded
    }
}
