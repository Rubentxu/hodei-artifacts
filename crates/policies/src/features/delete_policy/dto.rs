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
