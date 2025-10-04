use std::sync::Arc;

use crate::features::evaluate_permissions::dto::AuthorizationResponse;
use crate::features::evaluate_permissions::error::EvaluatePermissionsResult;
use crate::features::evaluate_permissions::ports::{
    AuthorizationCache, AuthorizationLogger, AuthorizationMetrics, EntityResolverPort,
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
    entity_resolver: Arc<dyn EntityResolverPort>,

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
        entity_resolver: Arc<dyn EntityResolverPort>,
        cache: Option<CACHE>,
        logger: LOGGER,
        metrics: METRICS,
    ) -> Self {
        Self {
            iam_use_case,
            org_use_case,
            authorization_engine,
            entity_resolver,
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
            self.entity_resolver,
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
    entity_resolver: Option<Arc<dyn EntityResolverPort>>,
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
            entity_resolver: None,
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

    /// Set the entity resolver
    pub fn with_entity_resolver(mut self, entity_resolver: Arc<dyn EntityResolverPort>) -> Self {
        self.entity_resolver = Some(entity_resolver);
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
            self.entity_resolver.ok_or("Entity resolver is required")?,
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
        entity_resolver: Arc<dyn EntityResolverPort>,
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
            entity_resolver,
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
        entity_resolver: Arc<dyn EntityResolverPort>,
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
            entity_resolver,
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
        MockEntityResolver,
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
        let entity_resolver = Arc::new(MockEntityResolver::new());

        let container = EvaluatePermissionsContainerBuilder::new()
            .with_iam_use_case(iam_use_case)
            .with_org_use_case(org_use_case)
            .with_authorization_engine(authorization_engine)
            .with_entity_resolver(entity_resolver)
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
            .with_entity_resolver(Arc::new(MockEntityResolver::new()))
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
        let entity_resolver = Arc::new(MockEntityResolver::new());
        let logger = MockAuthorizationLogger::new();
        let metrics = MockAuthorizationMetrics::new();

        let container = factories::create_without_cache(
            iam_use_case,
            org_use_case,
            authorization_engine,
            entity_resolver,
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
        let entity_resolver = Arc::new(MockEntityResolver::new());
        let logger = MockAuthorizationLogger::new();
        let metrics = MockAuthorizationMetrics::new();

        let container = factories::create_with_cache(
            iam_use_case,
            org_use_case,
            authorization_engine,
            entity_resolver,
            cache,
            logger,
            metrics,
        );

        let _use_case = container.build_use_case();
        assert!(true); // If we get here, construction succeeded
    }
}
