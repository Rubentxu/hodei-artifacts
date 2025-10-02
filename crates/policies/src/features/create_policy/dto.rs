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
