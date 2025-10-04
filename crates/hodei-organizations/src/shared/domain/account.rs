use policies::shared::Hrn;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Option<Hrn>,
    pub attached_scps: HashSet<Hrn>,
}

impl Account {
    pub fn new(hrn: Hrn, name: String, parent_hrn: Option<Hrn>) -> Self {
        Self {
            hrn,
            name,
            parent_hrn,
            attached_scps: HashSet::new(),
        }
    }

    pub fn set_parent(&mut self, parent_hrn: Hrn) {
        self.parent_hrn = Some(parent_hrn);
    }

    pub fn attach_scp(&mut self, scp_hrn: Hrn) {
        self.attached_scps.insert(scp_hrn);
    }

    pub fn detach_scp(&mut self, scp_hrn: &Hrn) -> bool {
        self.attached_scps.remove(scp_hrn)
    }

    pub fn has_scp(&self, scp_hrn: &Hrn) -> bool {
        self.attached_scps.contains(scp_hrn)
    }
}
