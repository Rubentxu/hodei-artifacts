// crates/iam/src/features/delete_policy/dto.rs

use crate::infrastructure::errors::IamError;
use serde::{Deserialize, Serialize};
use shared::hrn::PolicyId;

/// Command to delete a policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePolicyCommand {
    /// ID of the policy to delete
    pub id: PolicyId,
    /// User performing the deletion
    pub deleted_by: String,
}

/// Response after deleting a policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePolicyResponse {
    /// ID of the deleted policy
    pub id: PolicyId,
    /// Success message
    pub message: String,
}

impl DeletePolicyCommand {
    /// Create a new delete policy command
    pub fn new(id: PolicyId, deleted_by: String) -> Self {
        Self { id, deleted_by }
    }

    /// Validate the command
    pub fn validate(&self) -> Result<(), IamError> {
        // Validate deleted_by
        if self.deleted_by.trim().is_empty() {
            return Err(IamError::InvalidInput(
                "Deleted by field cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

impl DeletePolicyResponse {
    /// Create a new delete policy response
    pub fn new(id: PolicyId) -> Self {
        Self {
            id,
            message: "Policy deleted successfully".to_string(),
        }
    }
}