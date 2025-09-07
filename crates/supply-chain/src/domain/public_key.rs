// crates/supply-chain/src/domain/public_key.rs

use shared::hrn::{Hrn, OrganizationId};
use shared::lifecycle::Lifecycle;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;

/// Representa una clave pública utilizada para verificar firmas.
/// Es un Agregado Raíz, ya que su confianza y ciclo de vida se gestionan de forma independiente.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKey {
    /// El HRN único de la clave.
    /// Formato: `hrn:hodei:supply-chain:global:<org_id>:public-key/<key_fingerprint>`
    pub hrn: Hrn,
    
    /// La organización propietaria de la clave.
    pub organization_hrn: OrganizationId,
    
    /// El material de la clave en un formato estándar (ej. PEM, JWK).
    pub key_material: String,
    
    /// El algoritmo de la clave (ej. "ecdsa-p256").
    pub algorithm: String,
    
    /// De dónde proviene esta clave y cómo se estableció su confianza.
    pub source: KeySource,
    
    /// Opcional: la clave solo es válida después de esta fecha.
    pub valid_after: Option<OffsetDateTime>,
    
    /// Opcional: la clave solo es válida antes de esta fecha.
    pub valid_until: Option<OffsetDateTime>,
    
    /// Información de auditoría y ciclo de vida.
    pub lifecycle: Lifecycle,
}

/// La fuente de una clave pública.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeySource {
    /// La clave fue emitida por una autoridad de certificación como Sigstore Fulcio.
    CertificateAuthority { issuer: String },
    /// La clave fue subida manualmente por un usuario.
    ManualUpload { uploader_hrn: Hrn },
}