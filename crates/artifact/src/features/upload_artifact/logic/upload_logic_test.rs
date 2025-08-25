//! Unit tests for upload_artifact logic modules
//! 
//! These tests validate the pure logic functions in isolation, following
//! the testing pyramid principle by focusing on business logic without
//! external dependencies.

#[cfg(test)]
mod upload_artifact_logic_tests {
    use crate::domain::model::{Artifact, ArtifactVersion, ArtifactChecksum};
    use crate::features::upload_artifact::command::UploadArtifactCommand;
    use crate::features::upload_artifact::logic::{
        validate::{validate_upload_command, validate_idempotency, IdempotencyCheck},
        dedupe::{check_deduplication, DeduplicationResult},
        use_case::{execute_upload_use_case, UploadResult},
        build_event::{
            build_artifact_uploaded_event, 
            build_artifact_upload_idempotent_event,
            validate_artifact_for_events
        }
    };
    use crate::error::ArtifactError;
    use shared::{UserId, RepositoryId, ArtifactId, IsoTimestamp};
    use uuid::Uuid;

    // Test fixtures
    fn create_valid_command() -> UploadArtifactCommand {
        UploadArtifactCommand {
            repository_id: RepositoryId::new(),
            version: ArtifactVersion::new("1.0.0"),
            file_name: "test-artifact.jar".to_string(),
            size_bytes: 1024,
            checksum: ArtifactChecksum::new("a".repeat(64)), // Valid SHA-256 hex
            user_id: UserId::new(),
            bytes: vec![0u8; 1024], // Match size_bytes
        }
    }

    fn create_test_artifact() -> Artifact {
        let user_id = UserId::new();
        Artifact {
            id: ArtifactId::new(),
            repository_id: RepositoryId::new(),
            version: ArtifactVersion::new("1.0.0"),
            file_name: "test-artifact.jar".to_string(),
            size_bytes: 1024,
            checksum: ArtifactChecksum::new("a".repeat(64)), // Valid SHA-256 hex
            created_at: IsoTimestamp::now(),
            created_by: user_id,
            coordinates: None,
        }
    }

    // Validation tests
    #[test]
    fn test_validate_upload_command_success() {
        let cmd = create_valid_command();
        
        let result = validate_upload_command(&cmd);
        
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_upload_command_empty_filename() {
        let mut cmd = create_valid_command();
        cmd.file_name = "".to_string();
        
        let result = validate_upload_command(&cmd);
        
        assert!(result.is_err());
        assert!(matches!(result, Err(ArtifactError::InvalidFileName)));
    }

    #[test]
    fn test_validate_upload_command_zero_size() {
        let mut cmd = create_valid_command();
        cmd.size_bytes = 0;
        
        let result = validate_upload_command(&cmd);
        
        assert!(result.is_err());
        assert!(matches!(result, Err(ArtifactError::InvalidUploadCommand { .. })));
    }

    #[test]
    fn test_validate_upload_command_invalid_checksum() {
        let mut cmd = create_valid_command();
        cmd.checksum = ArtifactChecksum::new("invalid"); // Too short
        
        let result = validate_upload_command(&cmd);
        
        assert!(result.is_err());
        assert!(matches!(result, Err(ArtifactError::InvalidChecksum { .. })));
    }

    #[test]
    fn test_validate_idempotency_no_existing_artifact() {
        let cmd = create_valid_command();
        
        let result = validate_idempotency(&cmd, &None);
        
        assert!(result.is_ok());
        if let Ok(IdempotencyCheck::CanProceed) = result {
            // Expected
        } else {
            panic!("Expected CanProceed");
        }
    }

    #[test]
    fn test_validate_idempotency_matching_checksum() {
        let cmd = create_valid_command();
        let existing_artifact = Some(create_test_artifact());
        
        let result = validate_idempotency(&cmd, &existing_artifact);
        
        assert!(result.is_ok());
        if let Ok(IdempotencyCheck::AlreadyExists { .. }) = result {
            // Expected
        } else {
            panic!("Expected AlreadyExists");
        }
    }

    #[test]
    fn test_validate_idempotency_different_checksum() {
        let mut cmd = create_valid_command();
        cmd.checksum = ArtifactChecksum::new("b".repeat(64)); // Different valid checksum
        let existing_artifact = Some(create_test_artifact());
        
        let result = validate_idempotency(&cmd, &existing_artifact);
        
        assert!(result.is_err());
        if let Err(ArtifactError::ChecksumConflict { .. }) = result {
            // Expected
        } else {
            panic!("Expected ChecksumConflict error");
        }
    }

    // Deduplication tests
    #[test]
    fn test_check_deduplication_no_existing() {
        let cmd = create_valid_command();
        
        let result = check_deduplication(&cmd, None);
        
        assert!(result.is_ok());
        assert!(matches!(result, Ok(DeduplicationResult::NotFound)));
    }

    #[test]
    fn test_check_deduplication_same_checksum() {
        let cmd = create_valid_command();
        let existing_artifact = create_test_artifact();
        
        let result = check_deduplication(&cmd, Some(existing_artifact.clone()));
        
        assert!(result.is_ok());
        if let Ok(DeduplicationResult::ExactDuplicate { artifact_id }) = result {
            assert_eq!(artifact_id, existing_artifact.id);
        } else {
            panic!("Expected ExactDuplicate for same checksum");
        }
    }

    #[test]
    fn test_check_deduplication_different_checksum() {
        let mut cmd = create_valid_command();
        cmd.checksum = ArtifactChecksum::new("b".repeat(64)); // Different valid checksum
        let existing_artifact = create_test_artifact();
        
        let result = check_deduplication(&cmd, Some(existing_artifact));
        
        assert!(result.is_ok());
        if let Ok(DeduplicationResult::Conflict { existing_checksum, new_checksum }) = result {
            assert_eq!(existing_checksum, "a".repeat(64));
            assert_eq!(new_checksum, "b".repeat(64));
        } else {
            panic!("Expected Conflict for different checksum");
        }
    }

    // Use case tests
    #[test]
    fn test_execute_upload_use_case_new_artifact() {
        let cmd = create_valid_command();
        
        let result = execute_upload_use_case(&cmd, None);
        
        assert!(result.is_ok());
        if let Ok(UploadResult::Created { artifact }) = result {
            assert_eq!(artifact.version.0, cmd.version.0);
            assert_eq!(artifact.checksum.0, cmd.checksum.0);
            assert_eq!(artifact.created_by, cmd.user_id);
        } else {
            panic!("Expected Created variant");
        }
    }

    #[test]
    fn test_execute_upload_use_case_existing_artifact() {
        let cmd = create_valid_command();
        let existing_artifact = create_test_artifact();
        
        let result = execute_upload_use_case(&cmd, Some(existing_artifact.clone()));
        
        assert!(result.is_ok());
        if let Ok(UploadResult::AlreadyExists { artifact_id }) = result {
            assert_eq!(artifact_id, existing_artifact.id);
        } else {
            panic!("Expected AlreadyExists variant");
        }
    }

    // Event building tests
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
        assert_eq!(envelope.data.sha256.unwrap(), artifact.checksum.0);
        assert!(envelope.data.size_bytes.is_some());
        assert_eq!(envelope.data.size_bytes.unwrap(), artifact.size_bytes);
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
    fn test_validate_artifact_for_events_empty_filename() {
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
