use serde::{Deserialize, Serialize};
use shared::{ArtifactId, UserId};

/// Query para obtener un artifact para descarga
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetArtifactQuery {
    pub artifact_id: ArtifactId,
    pub user_id: UserId,
    pub use_presigned_url: bool,
    pub presigned_expires_secs: Option<u64>,
    pub user_agent: Option<String>,
    pub client_ip: Option<String>,
}

impl GetArtifactQuery {
    pub fn new(artifact_id: ArtifactId, user_id: UserId) -> Self {
        Self {
            artifact_id,
            user_id,
            use_presigned_url: false,
            presigned_expires_secs: None,
            user_agent: None,
            client_ip: None,
        }
    }

    pub fn with_presigned(mut self, expires_secs: u64) -> Self {
        self.use_presigned_url = true;
        self.presigned_expires_secs = Some(expires_secs);
        self
    }

    pub fn with_user_agent(mut self, user_agent: String) -> Self {
        self.user_agent = Some(user_agent);
        self
    }

    pub fn with_client_ip(mut self, client_ip: String) -> Self {
        self.client_ip = Some(client_ip);
        self
    }
}

/// Respuesta del query de descarga
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetArtifactResponse {
    pub artifact_id: ArtifactId,
    pub file_name: String,
    pub size_bytes: u64,
    pub media_type: Option<String>,
    pub checksum: String,
    pub download_method: DownloadMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DownloadMethod {
    /// Descarga directa con bytes incluidos
    Direct { content: Vec<u8> },
    /// Descarga mediante URL presignada
    PresignedUrl { url: String, expires_at: String },
}
