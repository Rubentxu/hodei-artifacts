use serde::Serialize;

#[derive(Debug, Clone)]
pub struct ValidatePolicyQuery {
    pub policy_content: String,
}

impl ValidatePolicyQuery {
    pub fn new(policy_content: String) -> Self {
        Self { policy_content }
    }

    pub fn validate(&self) -> Result<(), ValidatePolicyValidationError> {
        if self.policy_content.trim().is_empty() {
            return Err(ValidatePolicyValidationError::EmptyContent);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationWarning {
    pub message: String,
    pub severity: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ValidatePolicyValidationError {
    #[error("policy content cannot be empty")]
    EmptyContent,
}
