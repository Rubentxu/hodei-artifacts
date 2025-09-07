// crates/repository/src/domain/storage.rs

use shared::hrn::{Hrn, OrganizationId};
use shared::lifecycle::Lifecycle;
use serde::{Serialize, Deserialize};

/// Representa una configuración de un backend de almacenamiento físico.
/// Es un Agregado Raíz, ya que puede ser gestionado de forma independiente.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBackend {
    /// HRN del backend de almacenamiento.
    /// Formato: `hrn:hodei:repository:<region>:<org_id>:storage-backend/<backend_name>`
    pub hrn: Hrn,

    /// La organización a la que pertenece.
    pub organization_hrn: OrganizationId,

    /// Nombre del backend.
    pub name: String,

    /// El tipo de almacenamiento (S3, local, etc.).
    pub storage_type: StorageType,

    /// Detalles de conexión (bucket, endpoint, credenciales), encriptados.
    /// Se usa un tipo contenedor genérico para representar datos encriptados.
    pub connection_details: Encrypted<serde_json::Value>,

    /// Información de auditoría y ciclo de vida.
    pub lifecycle: Lifecycle,
}

/// El tipo de backend de almacenamiento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageType { S3Compatible, FileSystem, AzureBlob }

/// Un tipo contenedor para representar datos que deben estar encriptados en reposo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Encrypted<T> {
    encrypted_blob: Vec<u8>,
    // ... metadatos de encriptación
    _phantom: std::marker::PhantomData<T>,
}