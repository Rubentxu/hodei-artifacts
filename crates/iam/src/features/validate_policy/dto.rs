use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ValidatePolicyRequest {
    pub policy: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ValidatePolicyResponse {
    pub is_valid: bool,
    pub errors: Vec<String>,
}
