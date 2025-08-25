use crate::domain::model::{Artifact, ArtifactChecksum};
use crate::error::ArtifactError;
use crate::features::upload_artifact::command::UploadArtifactCommand;
use shared::ArtifactId;

/// Resultado del use case de upload que encapsula la decisión de crear vs encontrar existente
#[derive(Debug, Clone)]
pub enum UploadResult {
    /// El artefacto ya existía (idempotencia)
    AlreadyExists { artifact_id: ArtifactId },
    /// Se creó un nuevo artefacto
    Created { artifact: Artifact },
}

impl UploadResult {
    pub fn artifact_id(&self) -> ArtifactId {
        match self {
            UploadResult::AlreadyExists { artifact_id } => *artifact_id,
            UploadResult::Created { artifact } => artifact.id,
        }
    }

    pub fn is_new_creation(&self) -> bool {
        matches!(self, UploadResult::Created { .. })
    }
}

/// Lógica pura del use case de upload
/// Implementa la decisión de crear vs encontrar existente basado en idempotencia
pub fn execute_upload_use_case(
    cmd: &UploadArtifactCommand,
    existing_artifact: Option<Artifact>,
) -> Result<UploadResult, ArtifactError> {
    // 1. Validar comando básico
    if cmd.file_name.is_empty() {
        return Err(ArtifactError::Repository("file_name cannot be empty".to_string()));
    }
    
    if cmd.size_bytes == 0 {
        return Err(ArtifactError::Repository("size_bytes must be greater than 0".to_string()));
    }

    // 2. Verificar idempotencia
    if let Some(existing) = existing_artifact {
        return Ok(UploadResult::AlreadyExists {
            artifact_id: existing.id,
        });
    }

    // 3. Crear nuevo artefacto
    let checksum = ArtifactChecksum(cmd.checksum.0.clone());
    let artifact = Artifact::new(
        cmd.repository_id,
        cmd.version.clone(),
        cmd.file_name.clone(),
        cmd.size_bytes,
        checksum,
        cmd.user_id,
    );

    Ok(UploadResult::Created { artifact })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::ArtifactVersion;
    use shared::{RepositoryId, UserId};
    use uuid::Uuid;

    fn create_test_command() -> UploadArtifactCommand {
        UploadArtifactCommand {
            repository_id: RepositoryId(Uuid::new_v4()),
            version: ArtifactVersion("1.0.0".to_string()),
            file_name: "test.jar".to_string(),
            size_bytes: 1024,
            checksum: ArtifactChecksum("abc123".to_string()),
            user_id: UserId(Uuid::new_v4()),
            bytes: vec![1, 2, 3, 4],
        }
    }

    #[test]
    fn test_execute_upload_use_case_creates_new_artifact() {
        let cmd = create_test_command();
        let result = execute_upload_use_case(&cmd, None).unwrap();
        
        match &result {
            UploadResult::Created { artifact } => {
                assert_eq!(artifact.repository_id, cmd.repository_id);
                assert_eq!(artifact.file_name, cmd.file_name);
                assert_eq!(artifact.size_bytes, cmd.size_bytes);
                assert!(result.is_new_creation());
            }
            _ => panic!("Expected Created result"),
        }
    }

    #[test]
    fn test_execute_upload_use_case_returns_existing_artifact() {
        let cmd = create_test_command();
        let existing = Artifact::new(
            cmd.repository_id,
            cmd.version.clone(),
            cmd.file_name.clone(),
            cmd.size_bytes,
            cmd.checksum.clone(),
            cmd.user_id,
        );
        let existing_id = existing.id;
        
        let result = execute_upload_use_case(&cmd, Some(existing)).unwrap();
        
        match &result {
            UploadResult::AlreadyExists { artifact_id } => {
                assert_eq!(*artifact_id, existing_id);
                assert!(!result.is_new_creation());
            }
            _ => panic!("Expected AlreadyExists result"),
        }
    }

    #[test]
    fn test_execute_upload_use_case_validates_empty_filename() {
        let mut cmd = create_test_command();
        cmd.file_name = "".to_string();
        
        let result = execute_upload_use_case(&cmd, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_upload_use_case_validates_zero_size() {
        let mut cmd = create_test_command();
        cmd.size_bytes = 0;
        
        let result = execute_upload_use_case(&cmd, None);
        assert!(result.is_err());
    }
}
