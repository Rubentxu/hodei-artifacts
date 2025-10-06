use std::sync::Arc;

use crate::features::evaluate_permissions::dto::AuthorizationResponse;
use crate::features::evaluate_permissions::error::EvaluatePermissionsResult;
use crate::features::evaluate_permissions::ports::{
    AuthorizationCache, AuthorizationLogger, AuthorizationMetrics,
};
use crate::features::evaluate_permissions::use_case::EvaluatePermissionsUseCase;
use async_trait::async_trait;
use kernel::Hrn;
use kernel::application::ports::authorization::{IamPolicyEvaluator, ScpEvaluator};

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
/// This container injects evaluators following the new agnostic architecture
pub struct EvaluatePermissionsContainer<CACHE, LOGGER, METRICS> {
    // Evaluators from bounded contexts
    iam_evaluator: Arc<dyn IamPolicyEvaluator>,
    scp_evaluator: Arc<dyn ScpEvaluator>,

    // Cross-cutting concerns
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
        iam_evaluator: Arc<dyn IamPolicyEvaluator>,
        scp_evaluator: Arc<dyn ScpEvaluator>,
        cache: Option<CACHE>,
        logger: LOGGER,
        metrics: METRICS,
    ) -> Self {
        Self {
            iam_evaluator,
            scp_evaluator,
            cache,
            logger,
            metrics,
        }
    }

    /// Build the EvaluatePermissionsUseCase with all dependencies injected
    pub fn build_use_case(self) -> EvaluatePermissionsUseCase<CACHE, LOGGER, METRICS> {
        EvaluatePermissionsUseCase::new(
            self.iam_evaluator,
            self.scp_evaluator,
            self.cache,
            self.logger,
            self.metrics,
        )
    }
}

/// Builder pattern for creating the dependency injection container
pub struct EvaluatePermissionsContainerBuilder<CACHE, LOGGER, METRICS> {
    iam_evaluator: Option<Arc<dyn IamPolicyEvaluator>>,
    scp_evaluator: Option<Arc<dyn ScpEvaluator>>,
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
            iam_evaluator: None,
            scp_evaluator: None,
            cache: None,
            logger: None,
            metrics: None,
        }
    }

    /// Set the IAM policy evaluator
    pub fn with_iam_evaluator(mut self, iam_evaluator: Arc<dyn IamPolicyEvaluator>) -> Self {
        self.iam_evaluator = Some(iam_evaluator);
        self
    }

    /// Set the SCP evaluator
    pub fn with_scp_evaluator(mut self, scp_evaluator: Arc<dyn ScpEvaluator>) -> Self {
        self.scp_evaluator = Some(scp_evaluator);
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
            self.iam_evaluator.ok_or("IAM evaluator is required")?,
            self.scp_evaluator.ok_or("SCP evaluator is required")?,
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

/// Factory functions for common configurations
pub mod factories {
    use super::*;

    /// Create a container without cache (simplest configuration)
    pub fn create_without_cache<LOGGER, METRICS>(
        iam_evaluator: Arc<dyn IamPolicyEvaluator>,
        scp_evaluator: Arc<dyn ScpEvaluator>,
        logger: LOGGER,
        metrics: METRICS,
    ) -> EvaluatePermissionsContainer<DummyCache, LOGGER, METRICS>
    where
        LOGGER: AuthorizationLogger,
        METRICS: AuthorizationMetrics,
    {
        EvaluatePermissionsContainer::new(
            iam_evaluator,
            scp_evaluator,
            Some(DummyCache),
            logger,
            metrics,
        )
    }

    /// Create a container with cache
    pub fn create_with_cache<CACHE, LOGGER, METRICS>(
        iam_evaluator: Arc<dyn IamPolicyEvaluator>,
        scp_evaluator: Arc<dyn ScpEvaluator>,
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
            iam_evaluator,
            scp_evaluator,
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

    // Mock implementation of the EffectivePoliciesQueryPort (shared kernel) trait
    struct MockEffectivePoliciesQueryService;

    #[async_trait::async_trait]
    impl kernel::application::ports::EffectivePoliciesQueryPort for MockEffectivePoliciesQueryService {
        async fn get_effective_policies(
            &self,
            _query: kernel::application::ports::EffectivePoliciesQuery,
        ) -> Result<
            kernel::application::ports::EffectivePoliciesResult,
            Box<dyn std::error::Error + Send + Sync>,
        > {
            use cedar_policy::PolicySet;
            Ok(kernel::application::ports::EffectivePoliciesResult {
                policies: PolicySet::new(),
                policy_count: 0,
            })
        }
    }

    /// Helper para crear evaluadores de prueba SOLO para tests del DI
    ///
    /// ⚠️ IMPORTANTE: En código de producción, los evaluadores deben
    /// construirse en el APPLICATION LEVEL (main.rs), NO en hodei-authorizer.
    fn create_test_evaluators() -> (Arc<dyn IamPolicyEvaluator>, Arc<dyn ScpEvaluator>) {
        use kernel::Hrn;
        use kernel::application::ports::authorization::EvaluationDecision;

        #[derive(Clone)]
        struct TestIamEvaluator;

        #[async_trait]
        impl IamPolicyEvaluator for TestIamEvaluator {
            async fn evaluate_iam_policies(
                &self,
                request: kernel::application::ports::authorization::EvaluationRequest,
            ) -> Result<
                kernel::application::ports::authorization::EvaluationDecision,
                kernel::application::ports::AuthorizationError,
            > {
                Ok(EvaluationDecision {
                    principal_hrn: request.principal_hrn,
                    action_name: request.action_name,
                    resource_hrn: request.resource_hrn,
                    decision: true,
                    reason: "Test IAM evaluator always allows".to_string(),
                })
            }
        }

        #[derive(Clone)]
        struct TestScpEvaluator;

        #[async_trait]
        impl ScpEvaluator for TestScpEvaluator {
            async fn evaluate_scps(
                &self,
                request: kernel::application::ports::authorization::EvaluationRequest,
            ) -> Result<
                kernel::application::ports::authorization::EvaluationDecision,
                kernel::application::ports::AuthorizationError,
            > {
                Ok(EvaluationDecision {
                    principal_hrn: request.principal_hrn,
                    action_name: request.action_name,
                    resource_hrn: request.resource_hrn,
                    decision: true,
                    reason: "Test SCP evaluator always allows".to_string(),
                })
            }
        }

        (Arc::new(TestIamEvaluator), Arc::new(TestScpEvaluator))
    }

    #[test]
    fn test_builder_pattern() {
        let (iam_evaluator, scp_evaluator) = create_test_evaluators();
        let cache = MockAuthorizationCache::new();
        let logger = MockAuthorizationLogger::new();
        let metrics = MockAuthorizationMetrics::new();

        let container = EvaluatePermissionsContainerBuilder::new()
            .with_iam_evaluator(iam_evaluator)
            .with_scp_evaluator(scp_evaluator)
            .with_cache(cache)
            .with_logger(logger)
            .with_metrics(metrics)
            .build();

        assert!(container.is_ok());
    }

    #[test]
    fn test_builder_missing_required_dependency() {
        let (_iam_evaluator, scp_evaluator) = create_test_evaluators();

        let result: Result<
            EvaluatePermissionsContainer<
                MockAuthorizationCache,
                MockAuthorizationLogger,
                MockAuthorizationMetrics,
            >,
            String,
        > = EvaluatePermissionsContainerBuilder::new()
            .with_scp_evaluator(scp_evaluator)
            .with_logger(MockAuthorizationLogger::new())
            .with_metrics(MockAuthorizationMetrics::new())
            .build();

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.contains("IAM evaluator is required") || e.contains("required"));
        }
    }

    #[test]
    fn test_factory_without_cache() {
        let (iam_evaluator, scp_evaluator) = create_test_evaluators();
        let logger = MockAuthorizationLogger::new();
        let metrics = MockAuthorizationMetrics::new();

        let container = EvaluatePermissionsContainer::<MockAuthorizationCache, _, _>::new(
            iam_evaluator,
            scp_evaluator,
            None,
            logger,
            metrics,
        );

        // Verify container has no cache
        let _use_case = container.build_use_case();
        // Further assertions can be added based on use case behavior
    }

    #[test]
    fn test_factory_with_cache() {
        let (iam_evaluator, scp_evaluator) = create_test_evaluators();
        let cache = MockAuthorizationCache::new();
        let logger = MockAuthorizationLogger::new();
        let metrics = MockAuthorizationMetrics::new();

        let container = EvaluatePermissionsContainer::new(
            iam_evaluator,
            scp_evaluator,
            Some(cache),
            logger,
            metrics,
        );

        // Verify container has cache
        let _use_case = container.build_use_case();
        // Further assertions can be added based on use case behavior
    }
}
