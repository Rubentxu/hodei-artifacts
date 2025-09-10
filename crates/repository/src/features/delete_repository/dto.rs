// crates/repository/src/features/delete_repository/dto.rs

use serde::{Deserialize, Serialize};
use shared::enums::Ecosystem;
use crate::domain::repository::{RepositoryType, DeploymentPolicy, CacheSettings, ProxyAuth, ResolutionOrder};

/// Comando para eliminar un repositorio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteRepositoryCommand {
    /// HRN del repositorio a eliminar
    pub repository_hrn: String,
    
    /// Indica si se debe forzar la eliminación incluso si el repositorio no está vacío
    pub force: bool,
}

/// Respuesta de eliminación de repositorio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteRepositoryResponse {
    /// HRN del repositorio eliminado
    pub hrn: String,
    
    /// Nombre del repositorio
    pub name: String,
    
    /// Tipo de repositorio
    pub repo_type: RepositoryType,
    
    /// Ecosistema de paquetes
    pub format: Ecosystem,
    
    /// Indica si la eliminación fue exitosa
    pub success: bool,
    
    /// Mensaje de confirmación
    pub message: String,
    
    /// Estadísticas finales del repositorio antes de ser eliminado
    pub final_stats: RepositoryStatsResponse,
    
    /// Información de auditoría
    pub deleted_by: String,
    pub deleted_at: time::OffsetDateTime,
}

/// Estadísticas del repositorio antes de eliminación
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryStatsResponse {
    /// Número total de artefactos en el repositorio
    pub artifact_count: u64,
    
    /// Tamaño total en bytes
    pub total_size_bytes: u64,
    
    /// Fecha del último artefacto subido
    pub last_artifact_uploaded_at: Option<time::OffsetDateTime>,
    
    /// Número de descargas totales
    pub total_downloads: u64,
}

// Conversiones desde el modelo de dominio
impl From<crate::domain::repository::Repository> for DeleteRepositoryResponse {
    fn from(repository: crate::domain::repository::Repository) -> Self {
        DeleteRepositoryResponse {
            hrn: repository.hrn.as_str().to_string(),
            name: repository.name.clone(),
            repo_type: repository.repo_type,
            format: repository.format,
            success: true,
            message: format!("Repository '{}' successfully deleted", repository.name),
            final_stats: RepositoryStatsResponse {
                artifact_count: 0, // TODO: Calcular desde base de datos
                total_size_bytes: 0,
                last_artifact_uploaded_at: None,
                total_downloads: 0,
            },
            deleted_by: "system".to_string(), // TODO: Obtener del contexto
            deleted_at: time::OffsetDateTime::now_utc(),
        }
    }
}