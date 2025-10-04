use serde::{Deserialize, Serialize};

/// Command to get effective SCPs for an entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEffectiveScpsCommand {
    /// HRN of the target entity (Account or OU)
    pub target_hrn: String,
}

/// View of effective SCPs for an entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveScpsView {
    /// HRN of the target entity
    pub target_hrn: String,
    /// List of effective SCP HRNs
    pub effective_scps: Vec<String>,
}
