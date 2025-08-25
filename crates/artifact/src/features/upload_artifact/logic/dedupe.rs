//! Deduplication logic for upload artifact feature
//! 
//! This module contains functions for checking artifact existence and handling
//! deduplication logic, following Vertical Slice Architecture principles.

use crate::domain::model::{Artifact, ArtifactChecksum};
use crate::features::upload_artifact::command::UploadArtifactCommand;
use crate::error::ArtifactError;
use shared::ArtifactId;

/// Result of deduplication check
#[derive(Debug, Clone, PartialEq)]
pub enum DeduplicationResult {
    /// No existing artifact found, can proceed with creation
    NotFound,
    /// Exact duplicate found (same checksum) - idempotent case
    ExactDuplicate { artifact_id: ArtifactId },
    /// Conflict found (different checksum for same coordinates)
    Conflict { existing_checksum: String, new_checksum: String },
}

/// Performs deduplication check for upload command
/// 
/// This function encapsulates the logic for checking if an artifact already exists
/// and determining the appropriate action based on the comparison.
pub fn check_deduplication(
    cmd: &UploadArtifactCommand,
    existing_artifact: Option<Artifact>,
) -> Result<DeduplicationResult, ArtifactError> {
    match existing_artifact {
        None => Ok(DeduplicationResult::NotFound),
        Some(artifact) => {
            if artifact.checksum.0 == cmd.checksum.0 {
                // Exact match - idempotent case
                Ok(DeduplicationResult::ExactDuplicate {
                    artifact_id: artifact.id,
                })
            } else {
                // Different checksum - conflict
                Ok(DeduplicationResult::Conflict {
                    existing_checksum: artifact.checksum.0.clone(),
                    new_checksum: cmd.checksum.0.clone(),
                })
            }
        }
    }
}

/// Builds the checksum for repository lookup
/// 
/// This function creates the checksum object needed for repository queries.
/// It's a simple adapter function but provides a clear separation of concerns.
pub fn build_lookup_checksum(cmd: &UploadArtifactCommand) -> ArtifactChecksum {
    ArtifactChecksum(cmd.checksum.0.clone())
}

/// Determines if upload should proceed based on deduplication result
/// 
/// This function encapsulates the business logic for deciding whether to
/// proceed with artifact creation or handle the duplicate/conflict case.
pub fn should_proceed_with_upload(result: &DeduplicationResult) -> bool {
    matches!(result, DeduplicationResult::NotFound)
}

/// Extracts artifact ID from deduplication result for idempotent responses
/// 
/// Returns the artifact ID if the result represents an exact duplicate,
/// None otherwise.
pub fn extract_existing_artifact_id(result: &DeduplicationResult) -> Option<ArtifactId> {
    match result {
        DeduplicationResult::ExactDuplicate { artifact_id } => Some(artifact_id.clone()),
        _ => None,
    }
}

/// Converts deduplication conflict to appropriate error
/// 
/// This function handles the conversion of conflict cases to the appropriate
/// domain error for the application.
pub fn handle_deduplication_conflict(result: &DeduplicationResult) -> Result<(), ArtifactError> {
    match result {
        DeduplicationResult::Conflict { .. } => Err(ArtifactError::Duplicate),
        _ => Ok(()),
    }
}

