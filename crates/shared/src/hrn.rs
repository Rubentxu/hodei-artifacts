use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// ... (Hrn, HrnError, is_valid_organization_name, and hrn! macro remain the same)

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hrn(pub String);

impl Hrn {
    pub fn new(s: &str) -> Result<Self, HrnError> {
        if !s.starts_with("hrn:hodei:") {
            return Err(HrnError::InvalidFormat);
        }
        if s.starts_with("hrn:hodei:iam::system:organization/")
            && let Some(org_part) = s.split("organization/").nth(1) {
            let org_name = org_part.split('/').next().unwrap_or("");
            if !is_valid_organization_name(org_name) {
                return Err(HrnError::InvalidOrganizationName(org_name.to_string()));
            }
        }
        Ok(Hrn(s.to_string()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn organization_name(&self) -> Option<&str> {
        if self.0.starts_with("hrn:hodei:iam::system:organization/") {
            self.0.split("organization/").nth(1)?.split('/').next()
        } else {
            None
        }
    }
    pub fn resource_id(&self) -> Option<&str> {
        self.0.split('/').next_back()
    }
}

impl fmt::Display for Hrn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Hrn {
    type Err = HrnError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Hrn::new(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HrnError {
    InvalidFormat,
    InvalidOrganizationName(String),
}

impl fmt::Display for HrnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HrnError::InvalidFormat => write!(f, "Invalid HRN format"),
            HrnError::InvalidOrganizationName(name) => write!(f, "Invalid organization name: {}", name),
        }
    }
}

impl std::error::Error for HrnError {}

fn is_valid_organization_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 63 {
        return false;
    }
    let chars: Vec<char> = name.chars().collect();
    if chars.first() == Some(&'-') || chars.first() == Some(&'_') ||
       chars.last() == Some(&'-') || chars.last() == Some(&'_') {
        return false;
    }
    name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}


/// HRN para identificar organizaciones.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrganizationId(pub Hrn);

impl OrganizationId {
    pub fn new(name: &str) -> Result<Self, HrnError> {
        let hrn = Hrn::new(&format!("hrn:hodei:iam::system:organization/{}", name))?;
        Ok(OrganizationId(hrn))
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
    pub fn name(&self) -> Option<&str> {
        self.0.organization_name()
    }
}

impl fmt::Display for OrganizationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for OrganizationId {
    type Err = HrnError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(OrganizationId(Hrn::new(s)?))
    }
}

/// HRN para identificar repositorios.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepositoryId(pub Hrn);

impl RepositoryId {
    pub fn new(org_id: &str, name: &str) -> Result<Self, HrnError> {
        let hrn = Hrn::new(&format!("{}/repository/{}", org_id, name))?;
        Ok(RepositoryId(hrn))
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for RepositoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for RepositoryId {
    type Err = HrnError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(RepositoryId(Hrn::new(s)?))
    }
}

/// HRN para identificar artefactos físicos.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhysicalArtifactId(pub Hrn);

impl PhysicalArtifactId {
    pub fn new(hash: &str) -> Result<Self, HrnError> {
        let hrn = Hrn::new(&format!("hrn:hodei:artifact::physical-artifact/{}", hash))?;
        Ok(PhysicalArtifactId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}


/// HRN para identificar usuarios.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Hrn);

impl UserId {
    pub fn new(org_id: &OrganizationId, username: &str) -> Result<Self, HrnError> {
        let hrn = Hrn::new(&format!("{}/user/{}", org_id.as_str(), username))?;
        Ok(UserId(hrn))
    }
    pub fn new_system_user() -> Self {
        UserId(Hrn("hrn:hodei:iam::system:user/system".to_string()))
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
    type Err = HrnError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(UserId(Hrn::new(s)?))
    }
}

// ... (The rest of the file remains the same)