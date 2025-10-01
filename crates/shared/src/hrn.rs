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

/// HRN para identificar equipos.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamId(pub Hrn);

impl TeamId {
    pub fn new(org_id: &OrganizationId, team_name: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let hrn = Hrn::new(&format!("{}/team/{}", org_id.as_str(), team_name))?;
        Ok(TeamId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for TeamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for TeamId {
    type Err = Box<dyn std::error::Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(TeamId(Hrn::new(s)?))
    }
}

/// HRN para identificar repositorios.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepositoryId(pub Hrn);

impl RepositoryId {
    pub fn new(org_id: &OrganizationId, repo_name: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let hrn = Hrn::new(&format!("{}/repository/{}", org_id.as_str(), repo_name))?;
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
    type Err = Box<dyn std::error::Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(RepositoryId(Hrn::new(s)?))
    }
}

/// HRN para identificar artefactos.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactId(pub Hrn);

impl ArtifactId {
    pub fn new(org_id: &OrganizationId, repo_name: &str, artifact_name: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let hrn = Hrn::new(&format!("{}/repository/{}/artifact/{}", org_id.as_str(), repo_name, artifact_name))?;
        Ok(ArtifactId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ArtifactId {
    type Err = Box<dyn std::error::Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ArtifactId(Hrn::new(s)?))
    }
}

/// HRN para identificar artefactos físicos.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhysicalArtifactId(pub Hrn);

impl PhysicalArtifactId {
    pub fn new(org_id: &OrganizationId, repo_name: &str, physical_artifact_name: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let hrn = Hrn::new(&format!("{}/repository/{}/physical_artifact/{}", org_id.as_str(), repo_name, physical_artifact_name))?;
        Ok(PhysicalArtifactId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for PhysicalArtifactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for PhysicalArtifactId {
    type Err = Box<dyn std::error::Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PhysicalArtifactId(Hrn::new(s)?))
    }
}

/// HRN para identificar dashboards.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DashboardId(pub Hrn);

impl DashboardId {
    pub fn new(org_id: &OrganizationId, dashboard_name: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let hrn = Hrn::new(&format!("{}/dashboard/{}", org_id.as_str(), dashboard_name))?;
        Ok(DashboardId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for DashboardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for DashboardId {
    type Err = Box<dyn std::error::Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(DashboardId(Hrn::new(s)?))
    }
}

/// HRN para identificar reportes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReportId(pub Hrn);

impl ReportId {
    pub fn new(org_id: &OrganizationId, report_name: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let hrn = Hrn::new(&format!("{}/report/{}", org_id.as_str(), report_name))?;
        Ok(ReportId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for ReportId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ReportId {
    type Err = Box<dyn std::error::Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ReportId(Hrn::new(s)?))
    }
}

/// HRN para identificar alertas.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AlertId(pub Hrn);

impl AlertId {
    pub fn new(org_id: &OrganizationId, alert_name: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let hrn = Hrn::new(&format!("{}/alert/{}", org_id.as_str(), alert_name))?;
        Ok(AlertId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for AlertId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for AlertId {
    type Err = Box<dyn std::error::Error + Send + Sync>;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(AlertId(Hrn::new(s)?))
    }
}
