use serde::{Deserialize, Serialize};
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOuCommand {
    pub name: String,
    pub parent_hrn: Hrn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OuView {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
}
