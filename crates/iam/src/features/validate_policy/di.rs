// crates/iam/src/features/validate_policy/di.rs

use crate::infrastructure::errors::IamError;
use super::adapter::{
    CedarValidationAdapter, SimpleMetricsCollector, DefaultValidationConfigProvider
};
use super::api::ValidatePolicyApi;
use super::ports::{
    PolicyValidationService, PolicySyntaxValidator, PolicySemanticValidator,
    PolicyHrnValidator, ValidationMetricsCollector, ValidationConfigProvider
};
use super::use_case::ValidatePolicyUseCase;
use axum::Router;
use std::sync::Arc;

/// Dependency injection container for validate_policy feature
pub struct ValidatePolicyContainer {
    pub validation_service: Arc<dyn PolicyValidationService>,
    pub syntax_validator: Arc<dyn PolicySyntaxValidator>,
    pub semantic_validator: Arc<dyn PolicySemanticValidator>,
    pub hrn_validator: Arc<dyn PolicyHrnValidator>,
    pub metrics_collector: Arc<dyn ValidationMetricsCollector>,
    pub config_provider: Arc<dyn ValidationConfigProvider>,
}

impl ValidatePolicyContainer {
    /// Create a new container with default implementations
    pub fn new() -> Result<Self, IamError> {
        let cedar_adapter = Arc::new(CedarValidationAdapter::new()?);
        let metrics_collector = Arc::new(SimpleMetricsCollector::new());
        let config_provider = Arc::new(DefaultValidationConfigProvider::new());

        let validation_service = Arc::new(ValidatePolicyUseCase::new(
            cedar_adapter.clone(),
            cedar_adapter.clone(),
            cedar_adapter.clone(),
            metrics_collector.clone(),
            config_provider.clone(),
        ));

        Ok(Self {
            validation_service,
            syntax_validator: cedar_adapter.clone(),
            semantic_validator: cedar_adapter.clone(),
            hrn_validator: cedar_adapter,
            metrics_collector,
            config_provider,
        })
    }

    /// Create a new container with custom implementations
    pub fn with_custom_implementations(
        syntax_validator: Arc<dyn PolicySyntaxValidator>,
        semantic_validator: Arc<dyn PolicySemanticValidator>,
        hrn_validator: Arc<dyn PolicyHrnValidator>,
        metrics_collector: Arc<dyn ValidationMetricsCollector>,
        config_provider: Arc<dyn ValidationConfigProvider>,
    ) -> Self {
        let validation_service = Arc::new(ValidatePolicyUseCase::new(
            syntax_validator.clone(),
            semantic_validator.clone(),
            hrn_validator.clone(),
            metrics_collector.clone(),
            config_provider.clone(),
        ));

        Self {
            validation_service,
            syntax_validator,
            semantic_validator,
            hrn_validator,
            metrics_collector,
            config_provider,
        }
    }

    /// Get the validation service
    pub fn validation_service(&self) -> Arc<dyn PolicyValidationService> {
        self.validation_service.clone()
    }

    /// Create API router for this feature
    pub fn create_router(&self) -> Router {
        ValidatePolicyApi::router(self.validation_service.clone())
    }

    /// Create API instance
    pub fn create_api(&self) -> ValidatePolicyApi {
        ValidatePolicyApi::new(self.validation_service.clone())
    }
}

impl Default for ValidatePolicyContainer {
    fn default() -> Self {
        Self::new().expect("Failed to create default ValidatePolicyContainer")
    }
}

/// Builder for ValidatePolicyContainer
pub struct ValidatePolicyContainerBuilder {
    syntax_validator: Option<Arc<dyn PolicySyntaxValidator>>,
    semantic_validator: Option<Arc<dyn PolicySemanticValidator>>,
    hrn_validator: Option<Arc<dyn PolicyHrnValidator>>,
    metrics_collector: Option<Arc<dyn ValidationMetricsCollector>>,
    config_provider: Option<Arc<dyn ValidationConfigProvider>>,
}

impl ValidatePolicyContainerBuilder {
    pub fn new() -> Self {
        Self {
            syntax_validator: None,
            semantic_validator: None,
            hrn_validator: None,
            metrics_collector: None,
            config_provider: None,
        }
    }

    pub fn with_syntax_validator(mut self, validator: Arc<dyn PolicySyntaxValidator>) -> Self {
        self.syntax_validator = Some(validator);
        self
    }

    pub fn with_semantic_validator(mut self, validator: Arc<dyn PolicySemanticValidator>) -> Self {
        self.semantic_validator = Some(validator);
        self
    }

    pub fn with_hrn_validator(mut self, validator: Arc<dyn PolicyHrnValidator>) -> Self {
        self.hrn_validator = Some(validator);
        self
    }

    pub fn with_metrics_collector(mut self, collector: Arc<dyn ValidationMetricsCollector>) -> Self {
        self.metrics_collector = Some(collector);
        self
    }

    pub fn with_config_provider(mut self, provider: Arc<dyn ValidationConfigProvider>) -> Self {
        self.config_provider = Some(provider);
        self
    }

    pub fn build(self) -> Result<ValidatePolicyContainer, IamError> {
        // Use defaults for missing components
        let cedar_adapter = Arc::new(CedarValidationAdapter::new()?);
        
        let syntax_validator = self.syntax_validator.unwrap_or(cedar_adapter.clone());
        let semantic_validator = self.semantic_validator.unwrap_or(cedar_adapter.clone());
        let hrn_validator = self.hrn_validator.unwrap_or(cedar_adapter);
        let metrics_collector = self.metrics_collector.unwrap_or_else(|| Arc::new(SimpleMetricsCollector::new()));
        let config_provider = self.config_provider.unwrap_or_else(|| Arc::new(DefaultValidationConfigProvider::new()));

        Ok(ValidatePolicyContainer::with_custom_implementations(
            syntax_validator,
            semantic_validator,
            hrn_validator,
            metrics_collector,
            config_provider,
        ))
    }
}

impl Default for ValidatePolicyContainerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_creation() {
        let container = ValidatePolicyContainer::new();
        assert!(container.is_ok());
    }

    #[test]
    fn test_container_builder() {
        let container = ValidatePolicyContainerBuilder::new().build();
        assert!(container.is_ok());
    }

    #[test]
    fn test_container_router_creation() {
        let container = ValidatePolicyContainer::new().unwrap();
        let _router = container.create_router();
        // Router creation should not panic
    }

    #[test]
    fn test_container_api_creation() {
        let container = ValidatePolicyContainer::new().unwrap();
        let _api = container.create_api();
        // API creation should not panic
    }
}