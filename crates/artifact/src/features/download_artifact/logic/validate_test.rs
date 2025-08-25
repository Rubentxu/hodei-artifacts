#[cfg(test)]
mod tests {
    use crate::features::download_artifact::query::GetArtifactQuery;
    use crate::features::download_artifact::logic::validate::{
        validate_download_query, QueryValidationResult,
    };
    use crate::error::ArtifactError;
    use shared::{ArtifactId, UserId};
    use uuid::Uuid;

    fn create_valid_query() -> GetArtifactQuery {
        GetArtifactQuery {
            artifact_id: ArtifactId(Uuid::new_v4()),
            user_id: UserId(Uuid::new_v4()),
            use_presigned_url: false,
            presigned_expires_secs: None,
            user_agent: Some("test-agent".to_string()),
            client_ip: Some("127.0.0.1".to_string()),
        }
    }

    #[test]
    fn test_validate_download_query_valid() {
        let query = create_valid_query();
        let result = validate_download_query(&query);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), QueryValidationResult::Valid);
    }

    #[test]
    fn test_validate_download_query_with_presigned_valid() {
        let mut query = create_valid_query();
        query.use_presigned_url = true;
        query.presigned_expires_secs = Some(3600);

        let result = validate_download_query(&query);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_presigned_params_zero_expires() {
        let mut query = create_valid_query();
        query.use_presigned_url = true;
        query.presigned_expires_secs = Some(0);

        let result = validate_download_query(&query);
        assert!(result.is_err());
        match result.unwrap_err() {
            ArtifactError::InvalidUploadCommand { reason } => {
                assert!(reason.contains("cannot be zero"));
            }
            _ => panic!("Expected InvalidUploadCommand error"),
        }
    }

    #[test]
    fn test_validate_presigned_params_exceeds_limit() {
        let mut query = create_valid_query();
        query.use_presigned_url = true;
        query.presigned_expires_secs = Some(86401); // 24 hours + 1 second

        let result = validate_download_query(&query);
        assert!(result.is_err());
        match result.unwrap_err() {
            ArtifactError::InvalidUploadCommand { reason } => {
                assert!(reason.contains("cannot exceed 24 hours"));
            }
            _ => panic!("Expected InvalidUploadCommand error"),
        }
    }
}
