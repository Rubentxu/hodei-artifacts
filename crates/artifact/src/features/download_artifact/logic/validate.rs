use shared::{ArtifactId, UserId};
use crate::error::ArtifactError;
use super::super::query::GetArtifactQuery;

/// Resultado de validación de query de descarga
#[derive(Debug, PartialEq)]
pub enum QueryValidationResult {
    Valid,
}

/// Validación pura del query de descarga
pub fn validate_download_query(query: &GetArtifactQuery) -> Result<QueryValidationResult, ArtifactError> {
    // Validar artifact_id
    validate_artifact_id(&query.artifact_id)?;
    
    // Validar user_id
    validate_user_id(&query.user_id)?;
    
    // Validar parámetros de presigned URL
    if query.use_presigned_url {
        validate_presigned_params(query)?;
    }
    
    Ok(QueryValidationResult::Valid)
}

fn validate_artifact_id(_artifact_id: &ArtifactId) -> Result<(), ArtifactError> {
    // TODO: Implementar validaciones específicas si es necesario
    // Por ahora, solo verificamos que el ID no sea nulo (esto ya está garantizado por el tipo)
    Ok(())
}

fn validate_user_id(_user_id: &UserId) -> Result<(), ArtifactError> {
    // TODO: Implementar validaciones específicas si es necesario
    // Por ahora, solo verificamos que el ID no sea nulo (esto ya está garantizado por el tipo)
    Ok(())
}

fn validate_presigned_params(query: &GetArtifactQuery) -> Result<(), ArtifactError> {
    if let Some(expires_secs) = query.presigned_expires_secs {
        // Validar que el tiempo de expiración esté en un rango razonable
        if expires_secs == 0 {
            return Err(ArtifactError::InvalidUploadCommand {
                reason: "Expires seconds cannot be zero for presigned URLs".to_string(),
            });
        }
        
        // Máximo 24 horas
        if expires_secs > 86400 {
            return Err(ArtifactError::InvalidUploadCommand {
                reason: "Expires seconds cannot exceed 24 hours (86400 seconds)".to_string(),
            });
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
