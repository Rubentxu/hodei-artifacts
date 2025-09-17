use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hrn(pub String);

impl Hrn {
    pub fn new(s: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if !s.starts_with("hrn:hodei:") {
            return Err("Invalid HRN format - must start with 'hrn:hodei:'".into());
        }
        Ok(Hrn(s.to_string()))
    }
    
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Hrn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Hrn {
    type Err = Box<dyn std::error::Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Hrn::new(s)
    }
}

/// HRN para identificar organizaciones.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrganizationId(pub Hrn);

impl OrganizationId {
    pub fn new(name: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let hrn = Hrn::new(&format!("hrn:hodei:iam::system:organization/{}", name))?;
        Ok(OrganizationId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for OrganizationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for OrganizationId {
    type Err = Box<dyn std::error::Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(OrganizationId(Hrn::new(s)?))
    }
}

/// HRN para identificar políticas de Hodei.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HodeiPolicyId(pub Hrn);

impl HodeiPolicyId {
    pub fn new(org_id: &OrganizationId, policy_name: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let hrn = Hrn::new(&format!("{}/policy/{}", org_id.as_str(), policy_name))?;
        Ok(HodeiPolicyId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for HodeiPolicyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for HodeiPolicyId {
    type Err = Box<dyn std::error::Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(HodeiPolicyId(Hrn::new(s)?))
    }
}

/// HRN para identificar usuarios.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Hrn);

impl UserId {
    pub fn new(org_id: &OrganizationId, username: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let hrn = Hrn::new(&format!("{}/user/{}", org_id.as_str(), username))?;
        Ok(UserId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for UserId {
    type Err = Box<dyn std::error::Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(UserId(Hrn::new(s)?))
    }
}