//! Clean domain model for artifact bounded context
//! Pure DDD implementation without legacy compatibility

use shared::{
    hrn::Hrn,
    lifecycle::Lifecycle,
    enums::{Ecosystem, HashAlgorithm},
    models::UserHrn,
};
use time::OffsetDateTime;
use std::collections::HashMap;

/// Artifact aggregate root representing a logical artifact
#[derive(Debug, Clone)]
pub struct Artifact {
    /// Unique identifier using HRN
    pub id: ArtifactId,
    
    /// Repository this artifact belongs to
    pub repository_id: RepositoryId,
    
    /// Human-readable coordinates (name, version, qualifier)
    pub coordinates: ArtifactCoordinates,
    
    /// Current lifecycle state
    pub state: ArtifactState,
    
    /// Rich metadata
    pub metadata: ArtifactMetadata,
    
    /// User who uploaded this artifact
    pub uploader: UserHrn,
    
    /// Type of artifact (Maven, NPM, etc.)
    pub artifact_type: Ecosystem,
    
    /// Associated physical artifact
    pub physical_artifact: PhysicalArtifact,
    
    /// Associated SBOM
    pub sbom: Option<SoftwareBillOfMaterials>,
    
    /// Dependencies and relationships
    pub dependencies: Vec<ArtifactDependency>,
    pub reverse_dependencies: Vec<ArtifactId>,
    
    /// Security and compliance
    pub security_score: Option<SecurityScore>,
    pub compliance_status: ComplianceStatus,
    pub vulnerability_count: u32,
    
    /// Distribution information
    pub distribution_channels: Vec<DistributionChannel>,
    
    /// Lifecycle tracking
    pub lifecycle: Lifecycle,
    
    /// Timestamps
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub published_at: Option<OffsetDateTime>,
    pub archived_at: Option<OffsetDateTime>,
}

impl Artifact {
    /// Business logic: publish artifact
    pub fn publish(&mut self, publisher: UserHrn) -> Result<(), String> {
        if self.state != ArtifactState::Pending {
            return Err("Can only publish pending artifacts".to_string());
        }

        self.state = ArtifactState::Published;
        self.published_at = Some(OffsetDateTime::now_utc());
        self.updated_at = OffsetDateTime::now_utc();

        Ok(())
    }

    /// Business logic: deprecate artifact
    pub fn deprecate(&mut self, reason: String, deprecator: UserHrn) -> Result<(), String> {
        if self.state == ArtifactState::Deprecated {
            return Err("Artifact is already deprecated".to_string());
        }

        self.state = ArtifactState::Deprecated;
        self.metadata.add_custom_metadata("deprecation_reason".to_string(), reason);
        self.updated_at = OffsetDateTime::now_utc();

        Ok(())
    }
}

/// Artifact ID value object
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactId(pub Hrn);

/// Repository ID value object
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepositoryId(pub Hrn);

/// Unified artifact coordinates
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactCoordinates {
    pub name: String,
    pub version: String,
    pub qualifier: String,
}

impl ArtifactCoordinates {
    pub fn new(name: String, version: String, qualifier: String) -> Result<Self, String> {
        if name.trim().is_empty() {
            return Err("Name cannot be empty".to_string());
        }
        if version.trim().is_empty() {
            return Err("Version cannot be empty".to_string());
        }

        Ok(Self {
            name: name.trim().to_string(),
            version: version.trim().to_string(),
            qualifier: qualifier.trim().to_string(),
        })
    }

    pub fn to_string(&self) -> String {
        if self.qualifier.is_empty() {
            format!("{}: {}", self.name, self.version)
        } else {
            format!("{}:{}:{}", self.name, self.version, self.qualifier)
        }
    }
}

/// Unified artifact state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactState {
    Pending,
    Published,
    Deprecated,
    Archived,
}

/// Unified physical artifact
#[derive(Debug, Clone)]
pub struct PhysicalArtifact {
    pub id: PhysicalArtifactId,
    pub content_hash: ContentHash,
    pub size_in_bytes: u64,
    pub mime_type: String,
    pub storage_backend: String,
    pub storage_key: String,
    pub organization_id: Hrn,
    pub created_at: OffsetDateTime,
}

impl PhysicalArtifact {
    pub fn from_legacy(legacy: LegacyPhysicalArtifact) -> Self {
        Self {
            id: PhysicalArtifactId(legacy.hrn),
            content_hash: ContentHash {
                algorithm: legacy.content_hash.algorithm,
                value: legacy.content_hash.value,
            },
            size_in_bytes: legacy.size_in_bytes,
            mime_type: legacy.mime_type,
            storage_backend: legacy.storage_backend,
            storage_key: legacy.storage_key,
            organization_id: legacy.organization_hrn,
            created_at: legacy.created_at,
        }
    }

    pub fn to_legacy(&self) -> LegacyPhysicalArtifact {
        LegacyPhysicalArtifact {
            hrn: self.id.0.clone(),
            content_hash: crate::domain::physical_artifact::ContentHash {
                algorithm: self.content_hash.algorithm,
                value: self.content_hash.value.clone(),
            },
            size_in_bytes: self.size_in_bytes,
            mime_type: self.mime_type.clone(),
            storage_backend: self.storage_backend.clone(),
            storage_key: self.storage_key.clone(),
            organization_hrn: self.organization_id.clone(),
            created_at: self.created_at,
        }
    }
}

/// Physical artifact ID value object
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PhysicalArtifactId(pub Hrn);

/// Content hash value object
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentHash {
    pub algorithm: HashAlgorithm,
    pub value: String,
}

/// Unified artifact metadata
#[derive(Debug, Clone, Default)]
pub struct ArtifactMetadata {
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub documentation: Option<String>,
    pub repository_url: Option<String>,
    pub authors: Vec<String>,
    pub custom_metadata: HashMap<String, String>,
}

impl ArtifactMetadata {
    pub fn from_legacy(legacy: crate::domain::artifact::ArtifactMetadata) -> Self {
        Self {
            description: legacy.description,
            tags: legacy.tags,
            license: legacy.license,
            homepage: legacy.homepage,
            documentation: legacy.documentation,
            repository_url: legacy.repository_url,
            authors: legacy.authors,
            custom_metadata: legacy.custom_metadata,
        }
    }

    pub fn to_legacy(&self) -> crate::domain::artifact::ArtifactMetadata {
        crate::domain::artifact::ArtifactMetadata {
            description: self.description.clone(),
            tags: self.tags.clone(),
            license: self.license.clone(),
            homepage: self.homepage.clone(),
            documentation: self.documentation.clone(),
            repository_url: self.repository_url.clone(),
            authors: self.authors.clone(),
            custom_metadata: self.custom_metadata.clone(),
        }
    }

    pub fn add_tag(&mut self, tag: String) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }
    }

    pub fn remove_tag(&mut self, tag: &str) {
        self.tags.retain(|t| t != tag);
    }

    pub fn add_custom_metadata(&mut self, key: String, value: String) {
        self.custom_metadata.insert(key, value);
    }

    pub fn get_custom_metadata(&self, key: &str) -> Option<&String> {
        self.custom_metadata.get(key)
    }
}

/// Unified SBOM representation
#[derive(Debug, Clone)]
pub struct SoftwareBillOfMaterials {
    pub id: SbomId,
    pub artifact_id: ArtifactId,
    pub format: SbomFormat,
    pub version: String,
    pub content: Vec<u8>,
    pub signatures: Vec<SbomSignature>,
    pub metadata: HashMap<String, String>,
    pub generated_at: OffsetDateTime,
    pub generated_by: Option<String>,
    pub lifecycle: Lifecycle,
}

/// SBOM ID value object
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SbomId(pub Hrn);

/// SBOM format enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SbomFormat {
    SPDX,
    CycloneDX,
    Custom(String),
}

/// SBOM signature
#[derive(Debug, Clone)]
pub struct SbomSignature {
    pub signature: Vec<u8>,
    pub algorithm: String,
    pub key_id: Option<String>,
    pub signed_at: OffsetDateTime,
    pub expires_at: Option<OffsetDateTime>,
}

/// Artifact dependency relationship
#[derive(Debug, Clone)]
pub struct ArtifactDependency {
    pub artifact_id: ArtifactId,
    pub dependency_id: ArtifactId,
    pub version_constraint: String,
    pub scope: DependencyScope,
    pub is_optional: bool,
}

/// Dependency scope enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyScope {
    Compile,
    Runtime,
    Test,
    Provided,
    System,
}

/// Security score value object
#[derive(Debug, Clone)]
pub struct SecurityScore {
    pub overall_score: f32,
    pub vulnerability_score: f32,
    pub license_score: f32,
    pub quality_score: f32,
    pub last_calculated: OffsetDateTime,
}

/// Compliance status enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplianceStatus {
    Compliant,
    NonCompliant,
    UnderReview,
    Exempt,
}

/// Distribution channel value object
#[derive(Debug, Clone)]
pub struct DistributionChannel {
    pub channel_id: String,
    pub channel_name: String,
    pub region: String,
    pub is_active: bool,
    pub configuration: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::Hrn;

    #[test]
    fn test_artifact_coordinates() {
        let coords = ArtifactCoordinates::new(
            "my-library".to_string(),
            "1.0.0".to_string(),
            "release".to_string(),
        ).unwrap();
        
        assert_eq!(coords.name, "my-library");
        assert_eq!(coords.version, "1.0.0");
        assert_eq!(coords.qualifier, "release");
        assert_eq!(coords.to_string(), "my-library:1.0.0:release");
    }

    #[test]
    fn test_artifact_with_extended_attributes() {
        let hrn = Hrn::new("hrn:hodei:artifact:us-east-1:acme:artifact/my-lib/1.0.0").unwrap();
        let repo_hrn = Hrn::new("hrn:hodei:artifact:us-east-1:acme:repository/main").unwrap();
        let user_hrn = Hrn::new("hrn:hodei:iam:global:acme:user/dev").unwrap();
        let creator_hrn = Hrn::new("hrn:hodei:iam:global:acme:user/uploader").unwrap();
        
        let mut artifact = Artifact {
            id: ArtifactId(hrn.clone()),
            repository_id: RepositoryId(repo_hrn),
            coordinates: ArtifactCoordinates::new("my-lib".to_string(), "1.0.0".to_string(), "".to_string()).unwrap(),
            state: ArtifactState::Pending,
            metadata: ArtifactMetadata::default(),
            uploader: UserHrn(user_hrn.clone()),
            artifact_type: Ecosystem::Maven,
            physical_artifact: PhysicalArtifact {
                id: PhysicalArtifactId(Hrn::new("hrn:hodei:artifact:us-east-1:acme:physical-artifact/sha256-abc123").unwrap()),
                content_hash: ContentHash {
                    algorithm: HashAlgorithm::Sha256,
                    value: "abc123".to_string(),
                },
                size_in_bytes: 1024,
                mime_type: "application/java-archive".to_string(),
                storage_backend: "s3".to_string(),
                storage_key: "artifacts/my-lib-1.0.0.jar".to_string(),
                organization_id: Hrn::new("hrn:hodei:iam:global:acme:organization/acme").unwrap(),
                created_at: OffsetDateTime::now_utc(),
            },
            sbom: None,
            dependencies: Vec::new(),
            reverse_dependencies: Vec::new(),
            security_score: None,
            compliance_status: ComplianceStatus::UnderReview,
            vulnerability_count: 0,
            distribution_channels: Vec::new(),
            lifecycle: Lifecycle::new(creator_hrn),
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
            published_at: None,
            archived_at: None,
        };

        assert_eq!(artifact.state, ArtifactState::Pending);
        assert_eq!(artifact.compliance_status, ComplianceStatus::UnderReview);
        assert_eq!(artifact.vulnerability_count, 0);
        
        artifact.publish(UserHrn(user_hrn.clone())).unwrap();
        assert_eq!(artifact.state, ArtifactState::Published);
        assert!(artifact.published_at.is_some());
    }
}
