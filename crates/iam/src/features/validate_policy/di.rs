use std::sync::Arc;
use std::path::PathBuf;
use crate::features::validate_policy::adapter::CedarValidatorAdapter;
use crate::features::validate_policy::api::ValidatePolicyApi;
use crate::features::validate_policy::ports::PolicyValidatorPort;
use crate::features::validate_policy::use_case::ValidatePolicyUseCase;

// The Dependency Injection container for this feature.
pub struct ValidatePolicyDIContainer {
    pub api: Arc<ValidatePolicyApi>,
}

impl ValidatePolicyDIContainer {
    pub fn new(validator_port: Arc<dyn PolicyValidatorPort>) -> Self {
        let use_case = Arc::new(ValidatePolicyUseCase::new(validator_port));
        let api = Arc::new(ValidatePolicyApi::new(use_case));
        Self { api }
    }

    // Convenience method for production setup.
    pub fn for_production(schema_path: PathBuf) -> Result<Self, String> {
        let validator = Arc::new(CedarValidatorAdapter::new(schema_path).map_err(|e| e.to_string())?);
        Ok(Self::new(validator))
    }
    
    // In a real project, you would have a `for_testing` method here
    // that could inject a mock validator.
}
