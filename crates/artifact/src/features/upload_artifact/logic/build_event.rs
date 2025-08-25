//! Event building logic for upload artifact feature
//! 
//! This module encapsulates the construction of domain events following
//! Event-Carried State principles, ensuring events contain complete payloads
//! and proper correlation IDs for distributed tracing.

use crate::domain::model::Artifact;
use shared::domain::event::{ArtifactUploaded, DomainEventEnvelope};
use shared::domain::model::UserId;
use uuid::Uuid;

/// Builds an ArtifactUploaded event with complete state payload
/// 
/// Following Event-Carried State principles, this event contains all
/// necessary information for downstream consumers without requiring
/// additional data fetching.
/// 
/// # Arguments
/// * `artifact` - The uploaded artifact with complete metadata
/// * `correlation_id` - Unique identifier for tracing this operation
/// * `uploader` - User who uploaded the artifact
/// 
/// # Returns
/// A properly structured domain event envelope ready for publishing
pub fn build_artifact_uploaded_event(
    artifact: &Artifact,
    correlation_id: Uuid,
    uploader: &UserId,
) -> DomainEventEnvelope<ArtifactUploaded> {
    let payload = ArtifactUploaded {
        artifact_id: artifact.id.clone(),
        repository_id: artifact.repository_id.clone(),
        uploader: uploader.clone(),
        sha256: Some(artifact.checksum.0.clone()),
        size_bytes: Some(artifact.size_bytes),
        media_type: None, // TODO: extract from file_name extension or content
        upload_time_ms: None, // TODO: calculate from timing metrics
    };

    DomainEventEnvelope::from_correlation(
        payload,
        correlation_id,
        None, // causation_id
        Some("hodei-artifacts.artifact-upload".to_string()),
    )
}

/// Builds an event for when an upload is attempted but artifact already exists
/// 
/// This uses the same ArtifactUploaded event but with metadata indicating
/// it was an idempotent operation.
/// 
/// # Arguments
/// * `existing_artifact` - The existing artifact that was "re-uploaded"
/// * `correlation_id` - Unique identifier for tracing this operation
/// * `uploader` - User who attempted the upload
/// 
/// # Returns
/// A domain event envelope indicating an idempotent upload attempt
pub fn build_artifact_upload_idempotent_event(
    existing_artifact: &Artifact,
    correlation_id: Uuid,
    uploader: &UserId,
) -> DomainEventEnvelope<ArtifactUploaded> {
    let payload = ArtifactUploaded {
        artifact_id: existing_artifact.id.clone(),
        repository_id: existing_artifact.repository_id.clone(),
        uploader: uploader.clone(),
        sha256: Some(existing_artifact.checksum.0.clone()),
        size_bytes: Some(existing_artifact.size_bytes),
        media_type: None,
        upload_time_ms: None,
    };

    DomainEventEnvelope::from_correlation(
        payload,
        correlation_id,
        None,
        Some("hodei-artifacts.artifact-upload".to_string()),
    )
    .with_metadata("operation_type", "idempotent_upload")
    .with_metadata("reason", "artifact_already_exists")
}

/// Validates that an artifact has all required fields for event building
/// 
/// This is a pure validation function that ensures artifacts are in a
/// consistent state before event construction.
/// 
/// # Arguments
/// * `artifact` - The artifact to validate
/// 
/// # Returns
/// Ok(()) if valid, Err with validation message if invalid
pub fn validate_artifact_for_events(artifact: &Artifact) -> Result<(), String> {
    if artifact.id.0.to_string().is_empty() {
        return Err("Artifact ID cannot be empty".to_string());
    }
    
    if artifact.file_name.is_empty() {
        return Err("Artifact file_name cannot be empty".to_string());
    }
    
    if artifact.size_bytes == 0 {
        return Err("Artifact size must be greater than 0".to_string());
    }

    if artifact.checksum.0.is_empty() {
        return Err("Artifact checksum cannot be empty".to_string());
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{ArtifactChecksum, ArtifactVersion};
    use shared::domain::model::RepositoryId;

    fn create_test_artifact() -> Artifact {
        let created_by = UserId::new();
        Artifact::new(
            RepositoryId::new(),
            ArtifactVersion::new("1.0.0"),
            "test-artifact.jar".to_string(),
            1024,
            ArtifactChecksum::new("sha256:abc123"),
            created_by,
        )
    }

    #[test]
    fn test_build_artifact_uploaded_event() {
        let artifact = create_test_artifact();
        let correlation_id = Uuid::new_v4();
        let uploader = UserId::new();

        let envelope = build_artifact_uploaded_event(&artifact, correlation_id, &uploader);

        assert_eq!(envelope.event_type, "ArtifactUploaded.v1");
        assert_eq!(envelope.correlation_id, correlation_id);
        assert_eq!(envelope.data.artifact_id, artifact.id);
        assert_eq!(envelope.data.repository_id, artifact.repository_id);
        assert_eq!(envelope.data.uploader, uploader);
        assert!(envelope.data.sha256.is_some());
        assert!(envelope.data.size_bytes.is_some());
    }

    #[test]
    fn test_build_artifact_upload_idempotent_event() {
        let artifact = create_test_artifact();
        let correlation_id = Uuid::new_v4();
        let uploader = UserId::new();

        let envelope = build_artifact_upload_idempotent_event(&artifact, correlation_id, &uploader);

        assert_eq!(envelope.event_type, "ArtifactUploaded.v1");
        assert_eq!(envelope.correlation_id, correlation_id);
        assert_eq!(envelope.data.artifact_id, artifact.id);
        
        // Check metadata
        assert_eq!(envelope.metadata.get("operation_type"), Some(&"idempotent_upload".to_string()));
        assert_eq!(envelope.metadata.get("reason"), Some(&"artifact_already_exists".to_string()));
    }

    #[test]
    fn test_validate_artifact_for_events_valid() {
        let artifact = create_test_artifact();
        
        let result = validate_artifact_for_events(&artifact);
        
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_artifact_for_events_empty_file_name() {
        let mut artifact = create_test_artifact();
        artifact.file_name = "".to_string();
        
        let result = validate_artifact_for_events(&artifact);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("file_name cannot be empty"));
    }

    #[test]
    fn test_validate_artifact_for_events_zero_size() {
        let mut artifact = create_test_artifact();
        artifact.size_bytes = 0;
        
        let result = validate_artifact_for_events(&artifact);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("size must be greater than 0"));
    }

    #[test]
    fn test_validate_artifact_for_events_empty_checksum() {
        let mut artifact = create_test_artifact();
        artifact.checksum = ArtifactChecksum::new("");
        
        let result = validate_artifact_for_events(&artifact);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("checksum cannot be empty"));
    }
}
