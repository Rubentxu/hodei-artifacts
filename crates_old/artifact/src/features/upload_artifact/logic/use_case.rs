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