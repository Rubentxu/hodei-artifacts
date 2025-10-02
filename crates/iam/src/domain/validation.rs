// crates/iam/src/domain/validation.rs

use crate::infrastructure::errors::ValidationError;

/// Result of policy validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
}

impl ValidationResult {
    /// Create a successful validation result
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
        }
    }

    /// Create a failed validation result with errors
    pub fn invalid(errors: Vec<ValidationError>) -> Self {
        Self {
            is_valid: false,
            errors,
        }
    }

    /// Create a failed validation result with a single error
    pub fn invalid_with_message(message: String) -> Self {
        Self {
            is_valid: false,
            errors: vec![ValidationError {
                message,
                line: None,
                column: None,
            }],
        }
    }

    /// Add an error to the validation result
    pub fn add_error(&mut self, error: ValidationError) {
        self.errors.push(error);
        self.is_valid = false;
    }

    /// Get the first error message if any
    pub fn first_error_message(&self) -> Option<&str> {
        self.errors.first().map(|e| e.message.as_str())
    }
}
