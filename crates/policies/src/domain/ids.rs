use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use shared::hrn::Hrn;

/// Identificador HRN para políticas
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PolicyId(pub Hrn);

impl PolicyId {
    pub fn new(org_id: &shared::hrn::OrganizationId, name: &str) -> Result<Self, shared::hrn::HrnError> {
        let hrn = Hrn::new(&format!("{}/policy/{}", org_id.as_str(), name))?;
        Ok(PolicyId(hrn))
    }
    pub fn as_str(&self) -> &str { self.0.as_str() }
}

impl fmt::Display for PolicyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", self.0) }
}

impl FromStr for PolicyId {
    type Err = shared::hrn::HrnError;
    fn from_str(s: &str) -> Result<Self, Self::Err> { Ok(PolicyId(Hrn::new(s)?)) }
}
