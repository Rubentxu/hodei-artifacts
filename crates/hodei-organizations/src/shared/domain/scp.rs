use policies::shared::domain::hrn::Hrn;
use serde::{Deserialize, Serialize};

/// Represents a Service Control Policy in the organization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceControlPolicy {
    /// Unique identifier for the SCP
    pub hrn: Hrn,
    /// Name of the SCP
    pub name: String,
    /// Policy document in Cedar format
    pub document: String,
}

impl ServiceControlPolicy {
    /// Create a new Service Control Policy
    pub fn new(hrn: Hrn, name: String, document: String) -> Self {
        Self {
            hrn,
            name,
            document,
        }
    }
}
