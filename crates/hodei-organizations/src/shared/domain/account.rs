use policies::shared::Hrn;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
}

impl Account {
    pub fn new(hrn: Hrn, name: String, parent_hrn: Hrn) -> Self {
        Self { hrn, name, parent_hrn }
    }
    
    pub fn set_parent(&mut self, parent_hrn: Hrn) {
        self.parent_hrn = parent_hrn;
    }
    
    pub fn attach_scp(&mut self, _scp_hrn: Hrn) {
        // Placeholder for attaching an SCP
    }
}
