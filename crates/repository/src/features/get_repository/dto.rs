// crates/repository/src/features/get_repository/dto.rs

use serde::{Deserialize, Serialize};
use shared::enums::Ecosystem;
use shared::hrn::RepositoryId;
use crate::domain::repository::{RepositoryType, DeploymentPolicy, CacheSettings, ProxyAuth, ResolutionOrder};

/// Query para obtener un repositorio por su HRN
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetRepositoryQuery {
    /// HRN del repositorio a obtener
    pub repository_hrn: String,
}

/// Respuesta detallada de un repositorio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetRepositoryResponse {
    /// HRN único del repositorio
    pub hrn: String,
    
    /// Nombre del repositorio
    pub name: String,
    
    /// Descripción del repositorio
    pub description: Option<String>,
    
    /// Tipo de repositorio
    pub repo_type: RepositoryType,
    
    /// Ecosistema de paquetes
    pub format: Ecosystem,
    
    /// Configuración específica según el tipo
    pub config: RepositoryConfigResponse,
    
    /// Backend de almacenamiento (para Hosted)
    pub storage_backend_hrn: Option<String>,
    
    /// Información de ciclo de vida
    pub lifecycle: LifecycleResponse,
    
    /// Indica si el repositorio es público
    pub is_public: bool,
    
    /// Metadatos personalizados
    pub metadata: Option<std::collections::HashMap<String, String>>,
    
    /// Estadísticas del repositorio
    pub stats: RepositoryStatsResponse,
}

/// Configuración del repositorio para respuesta
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "config")]
pub enum RepositoryConfigResponse {
    Hosted(HostedConfigResponse),
    Proxy(ProxyConfigResponse),
    Virtual(VirtualConfigResponse),
}

/// Configuración para repositorio Hosted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedConfigResponse {
    pub deployment_policy: DeploymentPolicyResponse,
}

/// Configuración para repositorio Proxy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfigResponse {
    pub remote_url: String,
    pub cache_settings: CacheSettingsResponse,
    pub remote_authentication: Option<ProxyAuthResponse>,
}

/// Configuración para repositorio Virtual
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualConfigResponse {
    pub aggregated_repositories: Vec<String>,
    pub resolution_order: ResolutionOrderResponse,
}

/// Política de despliegue para respuesta
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DeploymentPolicyResponse {
    AllowSnapshots,
    BlockSnapshots,
    AllowRedeploy,
    BlockRedeploy,
}

/// Configuración de caché para respuesta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSettingsResponse {
    pub metadata_ttl_seconds: u32,
    pub artifact_ttl_seconds: u32,
}

/// Autenticación para proxy en respuesta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyAuthResponse {
    pub username: String,
    /// Nota: La contraseña nunca se incluye en las respuestas
    pub has_password: bool,
}

/// Orden de resolución para respuesta
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ResolutionOrderResponse {
    FirstFound,
}

/// Información de ciclo de vida
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleResponse {
    pub created_at: time::OffsetDateTime,
    pub created_by: String,
    pub updated_at: time::OffsetDateTime,
    pub updated_by: String,
}

/// Estadísticas del repositorio
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
impl From<DeploymentPolicy> for DeploymentPolicyResponse {
    fn from(policy: DeploymentPolicy) -> Self {
        match policy {
            DeploymentPolicy::AllowSnapshots => DeploymentPolicyResponse::AllowSnapshots,
            DeploymentPolicy::BlockSnapshots => DeploymentPolicyResponse::BlockSnapshots,
            DeploymentPolicy::AllowRedeploy => DeploymentPolicyResponse::AllowRedeploy,
            DeploymentPolicy::BlockRedeploy => DeploymentPolicyResponse::BlockRedeploy,
        }
    }
}

impl From<CacheSettings> for CacheSettingsResponse {
    fn from(settings: CacheSettings) -> Self {
        CacheSettingsResponse {
            metadata_ttl_seconds: settings.metadata_ttl_seconds,
            artifact_ttl_seconds: settings.artifact_ttl_seconds,
        }
    }
}

impl From<ProxyAuth> for ProxyAuthResponse {
    fn from(auth: ProxyAuth) -> Self {
        ProxyAuthResponse {
            username: auth.username,
            has_password: true, // Siempre true si existe el objeto
        }
    }
}

impl From<ResolutionOrder> for ResolutionOrderResponse {
    fn from(order: ResolutionOrder) -> Self {
        match order {
            ResolutionOrder::FirstFound => ResolutionOrderResponse::FirstFound,
        }
    }
}

impl From<crate::domain::repository::RepositoryConfig> for RepositoryConfigResponse {
    fn from(config: crate::domain::repository::RepositoryConfig) -> Self {
        match config {
            crate::domain::repository::RepositoryConfig::Hosted(hosted) => {
                RepositoryConfigResponse::Hosted(HostedConfigResponse {
                    deployment_policy: hosted.deployment_policy.into(),
                })
            },
            crate::domain::repository::RepositoryConfig::Proxy(proxy) => {
                RepositoryConfigResponse::Proxy(ProxyConfigResponse {
                    remote_url: proxy.remote_url.to_string(),
                    cache_settings: proxy.cache_settings.into(),
                    remote_authentication: proxy.remote_authentication.map(|auth| auth.into()),
                })
            },
            crate::domain::repository::RepositoryConfig::Virtual(virtual) => {
                RepositoryConfigResponse::Virtual(VirtualConfigResponse {
                    aggregated_repositories: virtual.aggregated_repositories
                        .iter()
                        .map(|repo_id| repo_id.as_str().to_string())
                        .collect(),
                    resolution_order: virtual.resolution_order.into(),
                })
            },
        }
    }
}

impl From<shared::lifecycle::Lifecycle> for LifecycleResponse {
    fn from(lifecycle: shared::lifecycle::Lifecycle) -> Self {
        LifecycleResponse {
            created_at: lifecycle.created_at,
            created_by: lifecycle.created_by,
            updated_at: lifecycle.updated_at,
            updated_by: lifecycle.updated_by,
        }
    }
}

impl From<crate::domain::repository::Repository> for GetRepositoryResponse {
    fn from(repository: crate::domain::repository::Repository) -> Self {
        GetRepositoryResponse {
            hrn: repository.hrn.as_str().to_string(),
            name: repository.name.clone(),
            description: None, // TODO: Agregar descripción al modelo de dominio
            repo_type: repository.repo_type,
            format: repository.format,
            config: repository.config.into(),
            storage_backend_hrn: Some(repository.storage_backend_hrn.clone()),
            lifecycle: repository.lifecycle.into(),
            is_public: false, // TODO: Agregar campo al modelo de dominio
            metadata: None,   // TODO: Agregar campo al modelo de dominio
            stats: RepositoryStatsResponse {
                artifact_count: 0, // TODO: Calcular desde base de datos
                total_size_bytes: 0,
                last_artifact_uploaded_at: None,
                total_downloads: 0,
            },
        }
    }
}