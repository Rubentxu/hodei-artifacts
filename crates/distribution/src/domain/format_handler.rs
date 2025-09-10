// crates/distribution/src/domain/format_handler.rs

use async_trait::async_trait;
use shared::hrn::{RepositoryId, UserId};
use shared::enums::Ecosystem;
use std::collections::HashMap;
use crate::domain::error::{DistributionError, DistributionResult};

/// Request genérico para operaciones de formato
#[derive(Debug, Clone)]
pub struct FormatRequest {
    /// Tipo de ecosistema (Maven, npm, Docker)
    pub ecosystem: Ecosystem,
    
    /// HRN del repositorio
    pub repository_hrn: RepositoryId,
    
    /// Usuario que realiza la operación
    pub user_id: UserId,
    
    /// Path del recurso solicitado
    pub path: String,
    
    /// Método HTTP
    pub method: String,
    
    /// Headers HTTP relevantes
    pub headers: HashMap<String, String>,
    
    /// Body de la petición (si aplica)
    pub body: Option<Vec<u8>>,
    
    /// Query parameters
    pub query_params: HashMap<String, String>,
}

/// Response genérico para operaciones de formato
#[derive(Debug, Clone)]
pub struct FormatResponse {
    /// Código de estado HTTP
    pub status_code: u16,
    
    /// Headers de respuesta
    pub headers: HashMap<String, String>,
    
    /// Body de la respuesta
    pub body: Option<Vec<u8>>,
    
    /// Content-Type
    pub content_type: Option<String>,
}

/// Trait base para todos los manejadores de formato
#[async_trait]
pub trait FormatHandler: Send + Sync {
    /// Procesa una petición de formato específico
    async fn handle_request(&self, request: FormatRequest) -> DistributionResult<FormatResponse>;
    
    /// Determina si este manejador puede procesar el path dado
    fn can_handle(&self, path: &str, ecosystem: &Ecosystem) -> bool;
    
    /// Obtiene el ecosistema que este manejador soporta
    fn supported_ecosystem(&self) -> Ecosystem;
    
    /// Genera metadatos del repositorio para este formato
    async fn generate_repository_metadata(
        &self,
        repository_hrn: &RepositoryId,
        user_id: &UserId,
    ) -> DistributionResult<FormatResponse>;
    
    /// Lista las versiones disponibles de un paquete
    async fn list_versions(
        &self,
        repository_hrn: &RepositoryId,
        package_name: &str,
        user_id: &UserId,
    ) -> DistributionResult<Vec<String>>;
    
    /// Obtiene información específica de una versión
    async fn get_version_info(
        &self,
        repository_hrn: &RepositoryId,
        package_name: &str,
        version: &str,
        user_id: &UserId,
    ) -> DistributionResult<FormatResponse>;
}

/// Metadata común para todos los formatos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub licenses: Vec<String>,
    pub dependencies: Vec<DependencyInfo>,
    pub published_at: Option<String>,
    pub download_url: Option<String>,
}

/// Información de dependencia
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyInfo {
    pub name: String,
    pub version_constraint: String,
    pub scope: String, // "compile", "runtime", "test", etc.
    pub is_optional: bool,
}

/// Información de versión para listados
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub version: String,
    pub published_at: String,
    pub is_latest: bool,
    pub download_count: u64,
}