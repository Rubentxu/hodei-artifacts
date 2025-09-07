// crates/iam/src/domain/api_key.rs

use shared::hrn::Hrn;
use shared::lifecycle::Lifecycle;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;

/// Representa una credencial de larga duración para acceso programático.
/// Es un Agregado Raíz.
/// Nota: Una ApiKey es una CREDENCIAL, no un principal. El sistema la usa para
/// identificar a su `owner` (User o ServiceAccount), que SÍ es el principal en la política.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// Un identificador corto, público y no secreto, usado para identificar la clave.
    pub id: String,

    /// El HRN único y global de la propia clave API.
    /// Formato: `hrn:hodei:iam:global:<org_id>:apikey/<key_id>`
    pub hrn: Hrn,
    
    /// El HRN de su propietario (un `User` o `ServiceAccount`). Este es el principal.
    pub owner_hrn: Hrn,
    
    /// El hash del token secreto. El token en texto plano solo se muestra una vez.
    pub hashed_token: String,
    
    /// Descripción de para qué se usa esta clave.
    pub description: Option<String>,

    /// Fecha y hora opcional de expiración.
    pub expires_at: Option<OffsetDateTime>,
    
    /// Información de auditoría y ciclo de vida.
    pub lifecycle: Lifecycle,
}