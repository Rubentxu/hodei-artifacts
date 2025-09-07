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