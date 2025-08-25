//! Validation logic for upload artifact feature
//! 
//! This module contains pure validation functions that check business rules
//! without side effects, following Vertical Slice Architecture principles.

use crate::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion};
use crate::error::ArtifactError;
use crate::features::upload_artifact::command::UploadArtifactCommand;
use shared::ArtifactId;

/// Validates the upload command against business rules
/// 
/// This is a pure function that performs validation without side effects.
/// It checks size limits, checksum format, and other business constraints.
pub fn validate_upload_command(cmd: &UploadArtifactCommand) -> Result<(), ArtifactError> {
    validate_file_size(cmd.size_bytes)?;
    validate_checksum_format(&cmd.checksum)?;
    validate_version(&cmd.version)?;
    validate_file_name(&cmd.file_name)?;
    Ok(())
}

/// Validates file size against configured limits
fn validate_file_size(size_bytes: u64) -> Result<(), ArtifactError> {
    const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100MB limit
    
    if size_bytes == 0 {
        return Err(ArtifactError::invalid_upload_command(
            "File size cannot be zero"
        ));
    }
    
    if size_bytes > MAX_FILE_SIZE {
        return Err(ArtifactError::file_size_exceeded(size_bytes, MAX_FILE_SIZE));
    }
    
    Ok(())
}

/// Validates checksum format (SHA-256 hex string)
fn validate_checksum_format(checksum: &ArtifactChecksum) -> Result<(), ArtifactError> {
    let checksum_str = &checksum.0;
    
    if checksum_str.is_empty() {
        return Err(ArtifactError::invalid_checksum(""));
    }
    
    // SHA-256 checksum should be 64 hex characters
    if checksum_str.len() != 64 {
        return Err(ArtifactError::invalid_checksum(format!(
            "Checksum must be exactly 64 characters long, got {}", 
            checksum_str.len()
        )));
    }
    
    // Check if all characters are valid hex
    if !checksum_str.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ArtifactError::invalid_checksum(
            "Checksum must contain only hexadecimal characters"
        ));
    }
    
    Ok(())
}

/// Validates version format
fn validate_version(version: &ArtifactVersion) -> Result<(), ArtifactError> {
    let version_str = &version.0;
    
    if version_str.is_empty() {
        return Err(ArtifactError::InvalidVersion { 
            version: "".to_string() 
        });
    }
    
    if version_str.len() > 50 {
        return Err(ArtifactError::InvalidVersion { 
            version: version_str.clone() 
        });
    }
    
    Ok(())
}

/// Validates file name format
fn validate_file_name(file_name: &str) -> Result<(), ArtifactError> {
    if file_name.is_empty() {
        return Err(ArtifactError::InvalidFileName);
    }
    
    if file_name.len() > 255 {
        return Err(ArtifactError::invalid_upload_command(
            "File name cannot exceed 255 characters"
        ));
    }
    
    // Check for invalid characters in file names
    if file_name.contains(&['/', '\\', ':', '*', '?', '"', '<', '>', '|'][..]) {
        return Err(ArtifactError::invalid_upload_command(
            "File name contains invalid characters"
        ));
    }
    
    Ok(())
}

/// Validates idempotency conditions based on existing artifact
/// 
/// This function implements the idempotency business rule:
/// - If artifact exists with same checksum → idempotent (return existing)
/// - If artifact exists with different checksum → conflict error
pub fn validate_idempotency(
    cmd: &UploadArtifactCommand,
    existing_artifact: &Option<Artifact>,
) -> Result<IdempotencyCheck, ArtifactError> {
    match existing_artifact {
        None => Ok(IdempotencyCheck::CanProceed),
        Some(artifact) => {
            if artifact.checksum.0 == cmd.checksum.0 {
                Ok(IdempotencyCheck::AlreadyExists {
                    artifact_id: artifact.id.clone(),
                })
            } else {
                Err(ArtifactError::checksum_conflict(
                    artifact.repository_id.0.to_string(),
                    artifact.checksum.0.clone(),
                    cmd.checksum.0.clone(),
                ))
            }
        }
    }
}

/// Result of idempotency validation
#[derive(Debug, Clone, PartialEq)]
pub enum IdempotencyCheck {
    /// No existing artifact, can proceed with creation
    CanProceed,
    /// Artifact already exists with same checksum (idempotent case)
    AlreadyExists { artifact_id: ArtifactId },
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{RepositoryId, UserId, ArtifactId, IsoTimestamp};

    fn create_valid_command() -> UploadArtifactCommand {
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

    #[test]
    fn test_validate_upload_command_success() {
        let cmd = create_valid_command();
        assert!(validate_upload_command(&cmd).is_ok());
    }

    #[test]
    fn test_validate_file_size_zero() {
        let mut cmd = create_valid_command();
        cmd.size_bytes = 0;
        
        let result = validate_upload_command(&cmd);
        assert!(matches!(result, Err(ArtifactError::InvalidUploadCommand { .. })));
    }

    #[test]
    fn test_validate_file_size_too_large() {
        let mut cmd = create_valid_command();
        cmd.size_bytes = 200 * 1024 * 1024; // 200MB > 100MB limit
        
        let result = validate_upload_command(&cmd);
        assert!(matches!(result, Err(ArtifactError::FileSizeExceeded { .. })));
    }

    #[test]
    fn test_validate_checksum_wrong_length() {
        let mut cmd = create_valid_command();
        cmd.checksum = ArtifactChecksum::new("abc123"); // Too short
        
        let result = validate_upload_command(&cmd);
        assert!(matches!(result, Err(ArtifactError::InvalidChecksum { .. })));
    }

    #[test]
    fn test_validate_checksum_invalid_hex() {
        let mut cmd = create_valid_command();
        cmd.checksum = ArtifactChecksum::new(format!("{}xyz", "a".repeat(61))); // Invalid hex chars
        
        let result = validate_upload_command(&cmd);
        assert!(matches!(result, Err(ArtifactError::InvalidChecksum { .. })));
    }

    #[test]
    fn test_validate_version_empty() {
        let mut cmd = create_valid_command();
        cmd.version = ArtifactVersion::new("");
        
        let result = validate_upload_command(&cmd);
        assert!(matches!(result, Err(ArtifactError::InvalidVersion { .. })));
    }

    #[test]
    fn test_validate_version_too_long() {
        let mut cmd = create_valid_command();
        cmd.version = ArtifactVersion::new("a".repeat(51)); // > 50 chars
        
        let result = validate_upload_command(&cmd);
        assert!(matches!(result, Err(ArtifactError::InvalidVersion { .. })));
    }

    #[test]
    fn test_validate_file_name_empty() {
        let mut cmd = create_valid_command();
        cmd.file_name = "".to_string();
        
        let result = validate_upload_command(&cmd);
        assert!(matches!(result, Err(ArtifactError::InvalidFileName)));
    }

    #[test]
    fn test_validate_file_name_invalid_chars() {
        let mut cmd = create_valid_command();
        cmd.file_name = "test/file.jar".to_string(); // Invalid slash
        
        let result = validate_upload_command(&cmd);
        assert!(matches!(result, Err(ArtifactError::InvalidUploadCommand { .. })));
    }

    #[test]
    fn test_validate_idempotency_no_existing() {
        let cmd = create_valid_command();
        let result = validate_idempotency(&cmd, &None);
        
        assert!(matches!(result, Ok(IdempotencyCheck::CanProceed)));
    }

    #[test]
    fn test_validate_idempotency_same_checksum() {
        let cmd = create_valid_command();
        let existing_artifact = Artifact {
            id: ArtifactId::new(),
            repository_id: cmd.repository_id.clone(),
            version: cmd.version.clone(),
            file_name: cmd.file_name.clone(),
            size_bytes: cmd.size_bytes,
            checksum: cmd.checksum.clone(),
            created_at: IsoTimestamp::now(),
            created_by: cmd.user_id.clone(),
            coordinates: None,
        };
        
        let result = validate_idempotency(&cmd, &Some(existing_artifact));
        assert!(matches!(result, Ok(IdempotencyCheck::AlreadyExists { .. })));
    }

    #[test]
    fn test_validate_idempotency_different_checksum() {
        let cmd = create_valid_command();
        let existing_artifact = Artifact {
            id: ArtifactId::new(),
            repository_id: cmd.repository_id.clone(),
            version: cmd.version.clone(),
            file_name: cmd.file_name.clone(),
            size_bytes: cmd.size_bytes,
            checksum: ArtifactChecksum::new("b".repeat(64)), // Different checksum
            created_at: IsoTimestamp::now(),
            created_by: cmd.user_id.clone(),
            coordinates: None,
        };
        
        let result = validate_idempotency(&cmd, &Some(existing_artifact));
        assert!(matches!(result, Err(ArtifactError::ChecksumConflict { .. })));
    }
}
