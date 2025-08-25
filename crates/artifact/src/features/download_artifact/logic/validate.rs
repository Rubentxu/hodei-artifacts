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

