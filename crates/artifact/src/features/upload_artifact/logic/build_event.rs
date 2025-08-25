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