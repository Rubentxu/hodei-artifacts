//! Module defining the Software Bill of Materials (SBOM) model

use serde::{Deserialize, Serialize};
use shared::hrn::{Hrn, PhysicalArtifactId};
use shared::lifecycle::Lifecycle;
use std::collections::HashMap;
use time::OffsetDateTime;

/// Represents a Software Bill of Materials (SBOM) for an artifact.
/// An SBOM is a formal record containing the details and supply chain relationships of various components used in building software.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sbom {
    /// Unique HRN identifier for this SBOM
    /// Format: `hrn:hodei:artifact:<region>:<org_id>:sbom/<hash_algorithm>-<hash_value>`
    pub hrn: Hrn,

    /// Reference to the physical artifact this SBOM describes
    pub physical_artifact_id: PhysicalArtifactId,

    /// Format of the SBOM (e.g., SPDX, CycloneDX)
    pub format: SbomFormat,

    /// Version of the SBOM format specification
    pub format_version: String,

    /// The actual SBOM content (serialized according to the specified format)
    pub content: Vec<u8>,

    /// Cryptographic signatures of this SBOM
    pub signatures: Vec<SbomSignature>,

    /// Additional metadata about the SBOM
    pub metadata: HashMap<String, String>,

    /// When this SBOM was generated
    pub generated_at: OffsetDateTime,

    /// Information about the tool that generated this SBOM
    pub generated_by: Option<String>,

    /// Version of the tool that generated this SBOM
    pub generated_using: Option<String>,

    /// Lifecycle information (creation, updates, etc.)
    pub lifecycle: Lifecycle,
}

/// Supported SBOM formats
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SbomFormat {
    /// SPDX format (https://spdx.dev/)
    Spdx,
    
    /// CycloneDX format (https://cyclonedx.org/)
    CycloneDx,
    
    /// Software Package Data Exchange (SPDX) in JSON format
    SpdxJson,
    
    /// Software Package Data Exchange (SPDX) in YAML format
    SpdxYaml,
    
    /// CycloneDX in JSON format
    CycloneDxJson,
    
    /// CycloneDX in XML format
    CycloneDxXml,
    
    /// Other format (specify in the string)
    Other(String),
}

/// Cryptographic signature of an SBOM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomSignature {
    /// The signature data
    pub signature: Vec<u8>,
    
    /// The algorithm used for the signature (e.g., "rsa-sha256", "ecdsa-sha384")
    pub algorithm: String,
    
    /// Optional identifier of the key used to create the signature
    pub key_id: Option<String>,
    
    /// When the signature was created
    pub signed_at: OffsetDateTime,
    
    /// Optional expiration time of the signature
    pub expires_at: Option<OffsetDateTime>,
}

impl Sbom {
    /// Creates a new SBOM instance.
    pub fn new(
        hrn: Hrn,
        physical_artifact_id: PhysicalArtifactId,
        format: SbomFormat,
        format_version: String,
        content: Vec<u8>,
        generated_by: Option<String>,
        generated_using: Option<String>,
        creator_hrn: Hrn,
    ) -> Self {
        Self {
            hrn,
            physical_artifact_id,
            format,
            format_version,
            content,
            signatures: Vec::new(),
            metadata: HashMap::new(),
            generated_at: OffsetDateTime::now_utc(),
            generated_by,
            generated_using,
            lifecycle: Lifecycle::new(creator_hrn),
        }
    }
    
    /// Adds a signature to this SBOM.
    pub fn add_signature(&mut self, signature: SbomSignature) {
        self.signatures.push(signature);
    }
    
    /// Adds a metadata key-value pair to this SBOM.
    pub fn add_metadata(&mut self, key: String, value: String) -> Option<String> {
        self.metadata.insert(key, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::Hrn;

    #[test]
    fn test_sbom_creation() {
        let hrn = Hrn::new("hrn:hodei:artifact:us-east-1:acme:sbom/sha256-abc123").unwrap();
        let artifact_hrn = Hrn::new("hrn:hodei:artifact:us-east-1:acme:physical-artifact/sha256-def456").unwrap();
        let creator_hrn = Hrn::new("hrn:hodei:iam:global:acme:user/sbom-generator").unwrap();
        
        let mut sbom = Sbom::new(
            hrn.clone(),
            PhysicalArtifactId(artifact_hrn),
            SbomFormat::Spdx,
            "2.2".to_string(),
            b"SPDX-2.2...".to_vec(),
            Some("sbom-tool".to_string()),
            Some("1.0.0".to_string()),
            creator_hrn,
        );
        
        assert_eq!(sbom.format, SbomFormat::Spdx);
        assert_eq!(sbom.format_version, "2.2");
        assert!(!sbom.content.is_empty());
        assert!(sbom.signatures.is_empty());
        
        // Test adding a signature
        let signature = SbomSignature {
            signature: vec![1, 2, 3, 4],
            algorithm: "rsa-sha256".to_string(),
            key_id: Some("key-123".to_string()),
            signed_at: OffsetDateTime::now_utc(),
            expires_at: None,
        };
        
        sbom.add_signature(signature);
        assert_eq!(sbom.signatures.len(), 1);
        
        // Test adding metadata
        sbom.add_metadata("author".to_string(), "John Doe".to_string());
        assert_eq!(sbom.metadata.get("author"), Some(&"John Doe".to_string()));
    }
}
