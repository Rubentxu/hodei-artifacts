// crates/shared/src/hrn.rs

use serde::{Serialize, Deserialize};
use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum HrnError {
    #[error("Invalid HRN format: {0}")]
    InvalidFormat(String),
}

/// Un HRN (Hodei Resource Name) validado, modelado a partir de los ARN de AWS.
/// Es el identificador canónico, único y global para cualquier recurso en Hodei.
/// El campo interno es privado para forzar la creación a través de constructores que garantizan la validez.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hrn(String);

impl Hrn {
    /// Construye un nuevo HRN, validando su formato.
    pub fn new(input: &str) -> Result<Self, HrnError> {
        // Basic validation
        if !input.starts_with("hrn:") {
            return Err(HrnError::InvalidFormat(input.to_string()));
        }
        
        // Validate HRN structure
        let parts: Vec<&str> = input.split(':').collect();
        if parts.len() < 5 {
            return Err(HrnError::InvalidFormat(input.to_string()));
        }
        
        // Validate organization name format (only for organization HRNs)
        if input.starts_with("hrn:hodei:iam::system:organization/") {
            if let Some(org_part) = input.split("organization/").nth(1) {
                let org_name = org_part.split('/').next().unwrap_or("");
                if !is_valid_organization_name(org_name) {
                    return Err(HrnError::InvalidFormat(input.to_string()));
                }
            }
        }
        
        Ok(Self(input.to_string()))
    }

    /// Devuelve el HRN como un string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Hrn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// --- Tipos de ID fuertemente tipados para seguridad de tipos en todo el sistema ---


/// Identificador para una `Organization`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrganizationId(pub Hrn);

impl OrganizationId {
    pub fn new(name: &str) -> Result<Self, HrnError> {
        let hrn_str = format!("hrn:hodei:iam::system:organization/{}", name);
        Ok(Self(Hrn::new(&hrn_str)?))
    }
}

/// Identificador para un `Repository`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepositoryId(pub Hrn);

impl RepositoryId {
    pub fn new(org_id: &OrganizationId, repo_name: &str) -> Result<Self, HrnError> {
        // Extract organization name from organization HRN
        let org_hrn = org_id.0.as_str();
        let org_name = org_hrn.split("organization/").nth(1).unwrap_or("");
        
        let hrn_str = format!("hrn:hodei:artifact::system:repository/{}/{}", org_name, repo_name);
        Ok(Self(Hrn::new(&hrn_str)?))
    }
}

/// Identificador para una `PackageVersion`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageVersionId(pub Hrn);

/// Identificador para un `PhysicalArtifact`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhysicalArtifactId(pub Hrn);

impl PhysicalArtifactId {
    pub fn new(content_hash: &str) -> Result<Self, HrnError> {
        let hrn_str = format!("hrn:hodei:artifact::system:physical-artifact/sha256-{}", content_hash);
        Ok(Self(Hrn::new(&hrn_str)?))
    }
}

/// Identificador para un `User`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Hrn);

impl UserId {
    pub fn new_system_user() -> Self {
        Self(Hrn("hrn:hodei:iam::system:user/system".to_string()))
    }

    /// Creates a UserId from an existing Hrn.
    /// This is useful when you have a Hrn that represents a user and need to convert it to UserId.
    pub fn from_hrn(hrn: Hrn) -> Self {
        Self(hrn)
    }
}

/// Identificador para una `ApiKey`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApiKeyId(pub Hrn);

/// Identificador para una `Policy` o `RetentionPolicy`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PolicyId(pub Hrn);

/// Identificador para una `PublicKey` usada en firmas.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicKeyId(pub Hrn);

/// Identificador para una `Attestation`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AttestationId(pub Hrn);

/// Identificador para un `SecurityScanResult`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScanResultId(pub Hrn);

/// Identificador para una `VulnerabilityDefinition`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VulnerabilityDefinitionId(pub Hrn);

/// Identificador para una `VulnerabilityOccurrence`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VulnerabilityOccurrenceId(pub Hrn);

/// Identificador para un `StorageBackend`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StorageBackendId(pub Hrn);

/// Validates if a string is a valid organization name.
/// Organization names should only contain alphanumeric characters, dots, and hyphens.
fn is_valid_organization_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 63 {
        return false;
    }
    
    // Check if name contains only allowed characters
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

impl PolicyId {
    /// Create a new PolicyId from a string
    pub fn new(id: &str) -> Result<Self, HrnError> {
        let hrn = if id.starts_with("hrn:") {
            Hrn::new(id)?
        } else {
            Hrn::new(&format!("hrn:hodei:iam:global:policy/{}", id))?
        };
        Ok(PolicyId(hrn))
    }

    /// Create a PolicyId from an existing Hrn
    pub fn from_hrn(hrn: Hrn) -> Self {
        PolicyId(hrn)
    }

    /// Get the underlying Hrn
    pub fn hrn(&self) -> &Hrn {
        &self.0
    }

    /// Get the policy name from the HRN
    pub fn name(&self) -> Option<&str> {
        self.0.0.split('/').last()
    }
}

impl std::fmt::Display for PolicyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for PolicyId {
    type Err = HrnError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}