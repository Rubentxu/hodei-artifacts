use serde::{Deserialize, Serialize};

/// Command to attach an SCP to an entity (Account or OU)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachScpCommand {
    /// HRN of the SCP to attach
    pub scp_hrn: String,
    /// HRN of the target entity (Account or OU)
    pub target_hrn: String,
}

/// View of the attach SCP operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachScpView {
    /// HRN of the SCP that was attached
    pub scp_hrn: String,
    /// HRN of the target entity
    pub target_hrn: String,
}
