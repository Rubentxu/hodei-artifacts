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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{ArtifactVersion};
    use shared::{RepositoryId, UserId, IsoTimestamp};

    fn create_test_command() -> UploadArtifactCommand {
        UploadArtifactCommand {
            repository_id: RepositoryId::new(),
            version: ArtifactVersion::new("1.0.0"),
            file_name: "test.jar".to_string(),
            size_bytes: 1024,
            checksum: ArtifactChecksum::new("a".repeat(64)),
            user_id: UserId::new(),
            bytes: vec![1, 2, 3, 4],
        }
    }

    fn create_test_artifact(cmd: &UploadArtifactCommand, checksum: ArtifactChecksum) -> Artifact {
        Artifact {
            id: ArtifactId::new(),
            repository_id: cmd.repository_id.clone(),
            version: cmd.version.clone(),
            file_name: cmd.file_name.clone(),
            size_bytes: cmd.size_bytes,
            checksum,
            created_at: IsoTimestamp::now(),
            created_by: cmd.user_id.clone(),
            coordinates: None,
        }
    }

    #[test]
    fn test_check_deduplication_not_found() {
        let cmd = create_test_command();
        let result = check_deduplication(&cmd, None).unwrap();
        
        assert_eq!(result, DeduplicationResult::NotFound);
        assert!(should_proceed_with_upload(&result));
        assert!(extract_existing_artifact_id(&result).is_none());
    }

    #[test]
    fn test_check_deduplication_exact_duplicate() {
        let cmd = create_test_command();
        let existing_artifact = create_test_artifact(&cmd, cmd.checksum.clone());
        let result = check_deduplication(&cmd, Some(existing_artifact.clone())).unwrap();
        
        assert!(matches!(result, DeduplicationResult::ExactDuplicate { .. }));
        assert!(!should_proceed_with_upload(&result));
        assert_eq!(extract_existing_artifact_id(&result), Some(existing_artifact.id));
        assert!(handle_deduplication_conflict(&result).is_ok());
    }

    #[test]
    fn test_check_deduplication_conflict() {
        let cmd = create_test_command();
        let different_checksum = ArtifactChecksum::new("b".repeat(64));
        let existing_artifact = create_test_artifact(&cmd, different_checksum.clone());
        let result = check_deduplication(&cmd, Some(existing_artifact)).unwrap();
        
        assert!(matches!(result, DeduplicationResult::Conflict { .. }));
        assert!(!should_proceed_with_upload(&result));
        assert!(extract_existing_artifact_id(&result).is_none());
        assert!(handle_deduplication_conflict(&result).is_err());
        
        if let DeduplicationResult::Conflict { existing_checksum, new_checksum } = result {
            assert_eq!(existing_checksum, different_checksum.0);
            assert_eq!(new_checksum, cmd.checksum.0);
        }
    }

    #[test]
    fn test_build_lookup_checksum() {
        let cmd = create_test_command();
        let checksum = build_lookup_checksum(&cmd);
        
        assert_eq!(checksum.0, cmd.checksum.0);
    }

    #[test]
    fn test_handle_deduplication_conflict_error() {
        let result = DeduplicationResult::Conflict {
            existing_checksum: "existing".to_string(),
            new_checksum: "new".to_string(),
        };
        
        let error_result = handle_deduplication_conflict(&result);
        assert!(matches!(error_result, Err(ArtifactError::Duplicate)));
    }

    #[test]
    fn test_handle_deduplication_conflict_success() {
        let results = vec![
            DeduplicationResult::NotFound,
            DeduplicationResult::ExactDuplicate { artifact_id: ArtifactId::new() },
        ];
        
        for result in results {
            assert!(handle_deduplication_conflict(&result).is_ok());
        }
    }
}
