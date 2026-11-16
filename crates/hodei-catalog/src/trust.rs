//! Las concesiones de confianza: que digest puede servir el canal trusted.
//!
//! Un repositorio sirve su contenido por uno de dos canales:
//!
//! - **`open`** (por defecto): lo publicado se sirve a quien Cedar deje
//!   leer. Es el canal de cuarentena: el contenido existe y se puede
//!   consultar, pero nadie ha dicho que sea de fiar.
//! - **`trusted`**: solo se sirve un digest que tiene una **concesion
//!   activa**, es decir, una decision de promocion que avanzo y que nadie
//!   ha revocado despues. Lo demas responde como si no existiera.
//!
//! La concesion es **por digest**, no por nombre. Una etiqueta que se
//! mueve apunta a otro digest, y ese digest no hereda nada: la decision de
//! ayer sobre `sha256:X` no dice nada sobre `sha256:Y`.
//!
//! # Por que se lee en vivo
//!
//! No hay cache de decisiones. Cada peticion al canal trusted pregunta al
//! libro, de modo que una revocacion tiene efecto en la peticion
//! siguiente, y cualquier error del libro es un error: **nunca** se
//! convierte en un permiso.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};

use crate::{CatalogError, RepositoryConfig};

/// El canal por el que un repositorio sirve su contenido.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServeChannel {
    /// Se sirve lo publicado, con independencia de la confianza.
    Open,
    /// Solo se sirve un digest con una concesion activa.
    TrustedOnly,
}

impl ServeChannel {
    /// El canal que declara la configuracion de un repositorio.
    ///
    /// Se lee de `config_settings.trust.serve`. Un repositorio sin la
    /// clave `trust` es abierto. Una clave `trust` que existe pero no se
    /// entiende (otro tipo, otro valor) es **trusted**: un ajuste de
    /// seguridad ilegible falla del lado cerrado, no del abierto.
    pub fn of(config: &RepositoryConfig) -> Self {
        let Some(trust) = config.config_settings.get("trust") else {
            return Self::Open;
        };
        match trust.get("serve").and_then(|serve| serve.as_str()) {
            Some("open") => Self::Open,
            _ => Self::TrustedOnly,
        }
    }

    /// El nombre estable del canal, tal y como va en la configuracion.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::TrustedOnly => "trusted",
        }
    }
}

/// La concesion de un digest dentro de un repositorio, por una decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustGrant {
    /// La cuenta propietaria del repositorio.
    pub account: String,
    /// El repositorio.
    pub repository: String,
    /// El contenido concedido.
    pub digest: ContentDigest,
    /// La decision de promocion que lo concedio.
    pub decision_id: String,
    /// Quien promovio.
    pub actor: String,
    /// Cuando se concedio.
    pub granted_at: DateTime<Utc>,
    /// Cuando se revoco, si se revoco.
    pub revoked_at: Option<DateTime<Utc>>,
    /// Quien revoco.
    pub revoked_by: Option<String>,
    /// Por que se revoco.
    pub revoke_reason: Option<String>,
}

impl TrustGrant {
    /// Si la concesion sigue en vigor.
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }
}

/// El libro de las concesiones.
///
/// No autoriza: quien decide si una persona puede promover o revocar es la
/// compuerta del servicio, antes de llegar aqui.
#[async_trait]
pub trait TrustLedger: Send + Sync {
    /// Escribe las concesiones de una decision, todas o ninguna.
    ///
    /// Repetir una concesion (misma cuenta, repositorio, digest y
    /// decision) la deja como estaba: la operacion es idempotente, y una
    /// concesion revocada **no** se reactiva por repetirla; para volver a
    /// conceder hace falta una decision nueva.
    async fn grant(&self, grants: &[TrustGrant]) -> Result<(), CatalogError>;

    /// Revoca todas las concesiones de una decision. Devuelve cuantas
    /// estaban activas.
    async fn revoke_decision(
        &self,
        account: &str,
        repository: &str,
        decision_id: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError>;

    /// Revoca todas las concesiones de un digest, sea cual sea la decision
    /// que las concedio. Devuelve cuantas estaban activas.
    async fn revoke_digest(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError>;

    /// Una concesion activa de ese digest en ese repositorio, si la hay.
    async fn active_grant(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
    ) -> Result<Option<TrustGrant>, CatalogError>;

    /// Todas las concesiones de un repositorio, activas o revocadas.
    async fn grants_of(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustGrant>, CatalogError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DigestScope;
    use serde_json::json;

    fn config(settings: serde_json::Value) -> RepositoryConfig {
        let mut config = RepositoryConfig::new(DigestScope::Global);
        config.config_settings = settings;
        config
    }

    #[test]
    fn a_repository_without_trust_settings_is_open() {
        assert_eq!(ServeChannel::of(&config(json!({}))), ServeChannel::Open);
    }

    #[test]
    fn the_trusted_channel_is_declared_by_name() {
        let trusted = config(json!({ "trust": { "serve": "trusted" } }));
        assert_eq!(ServeChannel::of(&trusted), ServeChannel::TrustedOnly);
        let open = config(json!({ "trust": { "serve": "open" } }));
        assert_eq!(ServeChannel::of(&open), ServeChannel::Open);
    }

    #[test]
    fn a_trust_setting_nobody_can_read_closes_the_channel() {
        for settings in [
            json!({ "trust": "trusted" }),
            json!({ "trust": {} }),
            json!({ "trust": { "serve": 1 } }),
            json!({ "trust": { "serve": "everybody" } }),
            json!({ "trust": { "serve": "TRUSTED" } }),
            json!({ "trust": null }),
        ] {
            assert_eq!(
                ServeChannel::of(&config(settings.clone())),
                ServeChannel::TrustedOnly,
                "{settings}"
            );
        }
    }
}
