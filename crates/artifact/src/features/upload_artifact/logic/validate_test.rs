#[cfg(test)]
mod tests {
    use crate::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion};
    use crate::features::upload_artifact::command::UploadArtifactCommand;
    use crate::features::upload_artifact::logic::validate::{
        validate_upload_command, validate_idempotency, IdempotencyCheck,
    };
    use crate::error::ArtifactError;
    use shared::{ArtifactId, IsoTimestamp, RepositoryId, UserId};

    fn create_valid_command() -> UploadArtifactCommand {
        UploadArtifactCommand {
            repository_id: RepositoryId::new(),
            version: ArtifactVersion::new("1.0.0"),
            file_name: "test.jar".to_string(),
            size_bytes: 1024,
            checksum: ArtifactChecksum::new("a".repeat(64)),
            user_id: UserId::new(),
            mime_type: "application/java-archive".to_string(),
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
