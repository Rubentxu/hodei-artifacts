#[derive(Debug, Clone)]
pub struct UpdatePolicyCommand {
    pub policy_id: String,
    pub new_policy_content: String,
}

impl UpdatePolicyCommand {
    pub fn new(policy_id: String, new_policy_content: String) -> Self {
        Self {
            policy_id,
            new_policy_content,
        }
    }

    pub fn validate(&self) -> Result<(), UpdatePolicyValidationError> {
        // Validar ID no vacío
        if self.policy_id.trim().is_empty() {
            return Err(UpdatePolicyValidationError::EmptyPolicyId);
        }

        // Validar contenido no vacío
        if self.new_policy_content.trim().is_empty() {
            return Err(UpdatePolicyValidationError::EmptyPolicyContent);
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UpdatePolicyValidationError {
    #[error("policy id cannot be empty")]
    EmptyPolicyId,
    #[error("policy content cannot be empty")]
    EmptyPolicyContent,
}
