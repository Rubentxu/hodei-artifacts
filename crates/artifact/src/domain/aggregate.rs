//! Artifact aggregate root and entities for the artifact bounded context

use crate::domain::events::ArtifactEvent;
use crate::domain::value_objects::{ArtifactCoordinates, ArtifactStatus};
use shared::hrn::Hrn;
use shared::lifecycle::Lifecycle;
use shared::models::UserHrn;
use shared::enums::Ecosystem;
use time::OffsetDateTime;

/// Artifact aggregate root representing a logical artifact
#[derive(Debug, Clone)]
pub struct Artifact {
    pub id: ArtifactId,
    pub repository_id: RepositoryId,
    pub coordinates: ArtifactCoordinates,
    pub status: ArtifactStatus,
    pub metadata: ArtifactMetadata,
    pub uploader_user_id: UserHrn,
    pub artifact_type: Ecosystem,
    pub physical_artifact_id: PhysicalArtifactId,
    pub lifecycle: Lifecycle,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl Artifact {
    pub fn new(
        id: ArtifactId,
        repository_id: RepositoryId,
        coordinates: ArtifactCoordinates,
        uploader_user_id: UserHrn,
        artifact_type: Ecosystem,
        physical_artifact_id: PhysicalArtifactId,
        creator_hrn: Hrn,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id,
            repository_id,
            coordinates,
            status: ArtifactStatus::Pending,
            metadata: ArtifactMetadata::default(),
            uploader_user_id,
            artifact_type,
            physical_artifact_id,
            lifecycle: Lifecycle::new(creator_hrn),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn publish(&mut self, publisher_hrn: Hrn) -> Result<Vec<ArtifactEvent>, String> {
        if self.status != ArtifactStatus::Pending {
            return Err("Artifact is not in pending status".to_string());
        }

        self.status = ArtifactStatus::Published;
        self.updated_at = OffsetDateTime::now_utc();
        
        Ok(vec![ArtifactEvent::ArtifactPublished {
            artifact_id: self.id.clone(),
            published_at: self.updated_at,
            published_by: publisher_hrn,
        }])
    }

    pub fn deprecate(&mut self, reason: String, deprecator_hrn: Hrn) -> Result<Vec<ArtifactEvent>, String> {
        if self.status == ArtifactStatus::Deprecated {
            return Err("Artifact is already deprecated".to_string());
        }

        self.status = ArtifactStatus::Deprecated;
        self.updated_at = OffsetDateTime::now_utc();
        
        Ok(vec![ArtifactEvent::ArtifactDeprecated {
            artifact_id: self.id.clone(),
            reason,
            deprecated_at: self.updated_at,
            deprecated_by: deprecator_hrn,
        }])
    }

    pub fn update_metadata(&mut self, metadata: ArtifactMetadata, updater_hrn: Hrn) -> Result<Vec<ArtifactEvent>, String> {
        self.metadata = metadata;
        self.updated_at = OffsetDateTime::now_utc();
        
        Ok(vec![ArtifactEvent::ArtifactMetadataUpdated {
            artifact_id: self.id.clone(),
            updated_at: self.updated_at,
            updated_by: updater_hrn,
        }])
    }
}

/// Artifact ID value object
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactId(pub Hrn);

/// Repository ID value object
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepositoryId(pub Hrn);

/// Physical Artifact ID value object
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PhysicalArtifactId(pub Hrn);

/// Artifact metadata value object
#[derive(Debug, Clone, Default)]
pub struct ArtifactMetadata {
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub documentation: Option<String>,
    pub repository_url: Option<String>,
    pub authors: Vec<String>,
    pub custom_metadata: std::collections::HashMap<String, String>,
}

impl ArtifactMetadata {
    pub fn add_tag(&mut self, tag: String) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }
    }

    pub fn add_custom_metadata(&mut self, key: String, value: String) {
        self.custom_metadata.insert(key, value);
    }
}
