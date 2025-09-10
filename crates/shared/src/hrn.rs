use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// HRN (Hodei Resource Name) - Un identificador único universal para recursos en Hodei.
/// 
/// Formato: hrn:hodei:{service}:{namespace}:{resource-type}/{resource-id}[/{sub-resource-type}/{sub-resource-id}]*
/// 
/// Ejemplos:
/// - hrn:hodei:iam::organization/acme
/// - hrn:hodei:artifact::repository/npm-registry/package/react/18.2.0
/// - hrn:hodei:security::sbom/npm-registry/package/react/18.2.0/sha256-abc123
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hrn(pub String);

impl Hrn {
    pub fn new(s: &str) -> Result<Self, HrnError> {
        // Basic validation
        if !s.starts_with("hrn:hodei:") {
            return Err(HrnError::InvalidFormat);
        }
        
        // Validate organization name if it's an organization HRN
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
    
    /// Extrae el nombre de la organización del HRN si es un HRN de organización.
    /// Devuelve None si no es un HRN de organización.
    pub fn organization_name(&self) -> Option<&str> {
        if self.0.starts_with("hrn:hodei:iam::system:organization/") {
            self.0.split("organization/").nth(1)?.split('/').next()
        } else {
            None
        }
    }
    
    /// Extrae el ID del recurso del HRN.
    /// Para un HRN como "hrn:hodei:artifact::repository/npm-registry/package/react/18.2.0",
    /// esto devolvería "18.2.0".
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

/// Valida que un nombre de organización cumpla con las restricciones:
/// - Solo caracteres alfanuméricos, guiones y guiones bajos
/// - Longitud entre 1 y 63 caracteres
/// - No puede comenzar ni terminar con guión o guión bajo
fn is_valid_organization_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 63 {
        return false;
    }
    
    let chars: Vec<char> = name.chars().collect();
    
    // No puede comenzar ni terminar con guión o guión bajo
    if chars.first() == Some(&'-') || chars.first() == Some(&'_') ||
       chars.last() == Some(&'-') || chars.last() == Some(&'_') {
        return false;
    }
    
    // Solo caracteres alfanuméricos, guiones y guiones bajos
    name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// Macro para crear HRNs de manera conveniente.
/// 
/// Ejemplo:
/// ```
/// # use shared::hrn::{hrn, Hrn};
/// let org_hrn = hrn!("iam", "", "organization/acme");
/// assert_eq!(org_hrn.as_str(), "hrn:hodei:iam::organization/acme");
/// ```
#[macro_export]
macro_rules! hrn {
    ($service:expr, $namespace:expr, $resource_path:expr) => {
        $crate::hrn::Hrn::new(&format!("hrn:hodei:{}:{}:{}", $service, $namespace, $resource_path)).unwrap()
    };
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

/// HRN para identificar repositorios.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepositoryId(pub Hrn);

impl RepositoryId {
    pub fn new(org_id: &OrganizationId, name: &str) -> Result<Self, HrnError> {
        let hrn = Hrn::new(&format!("{}/repository/{}", org_id.as_str(), name))?;
        Ok(RepositoryId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
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
    
    /// Crea un HRN para un usuario del sistema.
    pub fn new_system_user() -> Self {
        UserId(Hrn("hrn:hodei:iam::system:user/system".to_string()))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// HRN para identificar grupos.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId(pub Hrn);

impl GroupId {
    pub fn new(org_id: &OrganizationId, name: &str) -> Result<Self, HrnError> {
        let hrn = Hrn::new(&format!("{}/group/{}", org_id.as_str(), name))?;
        Ok(GroupId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// HRN para identificar políticas.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PolicyId(pub Hrn);

impl PolicyId {
    pub fn new(org_id: &OrganizationId, name: &str) -> Result<Self, HrnError> {
        let hrn = Hrn::new(&format!("{}/policy/{}", org_id.as_str(), name))?;
        Ok(PolicyId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// HRN para identificar sesiones.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Hrn);

impl SessionId {
    pub fn new(_org_id: &OrganizationId, user_id: &UserId) -> Result<Self, HrnError> {
        let uuid = Uuid::new_v4();
        let hrn = Hrn::new(&format!("{}/session/{}", user_id.as_str(), uuid))?;
        Ok(SessionId(hrn))
    }
    
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_valid_organization_hrn() {
        let hrn = Hrn::new("hrn:hodei:iam::system:organization/acme").unwrap();
        assert_eq!(hrn.as_str(), "hrn:hodei:iam::system:organization/acme");
        assert_eq!(hrn.organization_name(), Some("acme"));
    }
    
    #[test]
    fn test_invalid_organization_hrn() {
        let hrn = Hrn::new("hrn:hodei:iam::system:organization/-invalid-");
        assert!(hrn.is_err());
    }
    
    #[test]
    fn test_resource_id_extraction() {
        let hrn = Hrn::new("hrn:hodei:artifact::repository/npm-registry/package/react/18.2.0").unwrap();
        assert_eq!(hrn.resource_id(), Some("18.2.0"));
    }
    
    #[test]
    fn test_organization_id() {
        let org_id = OrganizationId::new("acme").unwrap();
        assert_eq!(org_id.as_str(), "hrn:hodei:iam::system:organization/acme");
        assert_eq!(org_id.name(), Some("acme"));
    }
    
    #[test]
    fn test_user_id() {
        let org_id = OrganizationId::new("acme").unwrap();
        let user_id = UserId::new(&org_id, "john_doe").unwrap();
        assert_eq!(user_id.as_str(), "hrn:hodei:iam::system:organization/acme/user/john_doe");
    }
    
    #[test]
    fn test_system_user_id() {
        let system_user = UserId::new_system_user();
        assert_eq!(system_user.as_str(), "hrn:hodei:iam::system:user/system");
    }
}