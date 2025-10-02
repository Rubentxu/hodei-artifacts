#[derive(Debug, Clone)]
pub struct GetPolicyQuery {
    pub policy_id: String,
}

impl GetPolicyQuery {
    pub fn new(policy_id: impl Into<String>) -> Self {
        Self {
            policy_id: policy_id.into(),
        }
    }

    pub fn validate(&self) -> Result<(), GetPolicyValidationError> {
        if self.policy_id.trim().is_empty() {
            return Err(GetPolicyValidationError::EmptyPolicyId);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GetPolicyValidationError {
    #[error("policy id cannot be empty")]
    EmptyPolicyId,
}
