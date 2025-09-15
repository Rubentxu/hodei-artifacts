// crates/artifact/src/features/upload_artifact/upload_progress/dto.rs

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Estado del progreso de una subida
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UploadStatus {
    /// Subida en progreso
    InProgress,
    /// Subida completada exitosamente
    Completed,
    /// Subida fallida
    Failed,
    /// Subida cancelada por el usuario
    Cancelled,
}

/// Información de progreso de una subida
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadProgress {
    /// ID único de la subida
    pub upload_id: String,
    /// Total de bytes esperados
    pub total_bytes: u64,
    /// Bytes transferidos hasta el momento
    pub bytes_transferred: u64,
    /// Porcentaje completado (0-100)
    pub percentage: u8,
    /// Estado actual de la subida
    pub status: UploadStatus,
    /// Timestamp de la última actualización
    pub last_updated: u64,
    /// Tiempo estimado restante en segundos (opcional)
    pub estimated_seconds_remaining: Option<u64>,
}

/// Response para consultas de progreso
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadProgressResponse {
    /// Información de progreso
    pub progress: UploadProgress,
    /// URL para seguir consultando (para polling)
    pub poll_url: Option<String>,
    /// URL de WebSocket para updates en tiempo real (opcional)
    pub websocket_url: Option<String>,
}

/// Comando para actualizar el progreso
#[derive(Debug, Clone)]
pub struct UpdateProgressCommand {
    pub upload_id: String,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub status: UploadStatus,
}

impl UploadProgress {
    /// Crear un nuevo progreso
    pub fn new(upload_id: String, total_bytes: u64) -> Self {
        Self {
            upload_id,
            total_bytes,
            bytes_transferred: 0,
            percentage: 0,
            status: UploadStatus::InProgress,
            last_updated: current_timestamp(),
            estimated_seconds_remaining: None,
        }
    }

    /// Actualizar el progreso con nuevos bytes transferidos
    pub fn update(&mut self, bytes_transferred: u64, total_bytes: u64) {
        self.bytes_transferred = bytes_transferred;
        self.total_bytes = total_bytes;
        self.percentage = if total_bytes > 0 {
            ((bytes_transferred as f64 / total_bytes as f64) * 100.0).min(100.0) as u8
        } else {
            0
        };
        self.last_updated = current_timestamp();

        // Calcular ETA simple (podría mejorarse con más datos históricos)
        self.estimated_seconds_remaining = self.calculate_eta();
    }

    /// Marcar como completado
    pub fn mark_completed(&mut self) {
        self.status = UploadStatus::Completed;
        self.bytes_transferred = self.total_bytes;
        self.percentage = 100;
        self.last_updated = current_timestamp();
        self.estimated_seconds_remaining = Some(0);
    }

    /// Marcar como fallido
    pub fn mark_failed(&mut self) {
        self.status = UploadStatus::Failed;
        self.last_updated = current_timestamp();
        self.estimated_seconds_remaining = None;
    }

    /// Calcular tiempo estimado restante
    fn calculate_eta(&self) -> Option<u64> {
        if self.bytes_transferred == 0 || self.percentage == 0 {
            return None;
        }

        let elapsed_time = current_timestamp() - (self.last_updated - 1000); // approx 1s ago
        if elapsed_time == 0 {
            return None;
        }

        let bytes_per_second = self.bytes_transferred / elapsed_time;
        if bytes_per_second == 0 {
            return None;
        }

        let remaining_bytes = self.total_bytes.saturating_sub(self.bytes_transferred);
        Some(remaining_bytes / bytes_per_second)
    }
}

/// Obtener timestamp actual en segundos
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Información sobre un chunk recibido
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceivedChunkInfo {
    /// Número del chunk (1-indexed)
    pub chunk_number: usize,
    /// Tamaño del chunk en bytes
    pub size: u64,
}

/// Response para consultas de chunks recibidos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceivedChunksResponse {
    /// ID único de la subida
    pub upload_id: String,
    /// Total de chunks esperados
    pub total_chunks: usize,
    /// Lista de chunks recibidos con su información
    pub received_chunks: Vec<ReceivedChunkInfo>,
    /// Números de los chunks recibidos (para fácil consulta)
    pub received_chunk_numbers: Vec<usize>,
}
