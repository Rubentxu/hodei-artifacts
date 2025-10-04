use serde::{Deserialize, Serialize};
use policies::domain::Hrn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScpCommand {
    pub name: String,
    pub document: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScpView {
    pub hrn: Hrn,
    pub name: String,
    pub document: String,
}
