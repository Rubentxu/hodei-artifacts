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
//! # Que dice una concesion de si misma
//!
//! Cada concesion nueva lleva la **vigencia** con la que nacio: el sujeto
//! evaluado, la version de la politica que decidio, el hash de la capsula
//! de evidencia y la version del riesgo que se consultaron, y el momento
//! hasta el que es digna de fe (`valid_until`). Con eso una concesion se
//! puede auditar y revalidar sin volver a preguntar al pasado; sin eso
//! **no es activa**: las filas escritas antes de este contrato siguen en
//! la base como historia, pero el canal trusted no las sirve hasta que una
//! promocion nueva las revalide. Fallar cerrado es la migracion.
//!
//! # Por que se lee en vivo
//!
//! Cada peticion al canal trusted pregunta al libro **con el reloj de
//! ahora**: una revocacion tiene efecto en la peticion siguiente, una
//! vigencia caducada deja de servir sin que nadie la revoque, y cualquier
//! error del libro es un error: **nunca** se convierte en un permiso.

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
    Trusted,
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
            _ => Self::Trusted,
        }
    }

    /// El nombre estable del canal, tal y como va en la configuracion.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Trusted => "trusted",
        }
    }

    /// The channel that says its own name.
    ///
    /// The name is the one that goes in the configuration: `open` or
    /// `trusted`. A name that does not exist is not a channel: it is a
    /// text nobody can interpret, and a channel that cannot be read is
    /// the closed one.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "open" => Some(Self::Open),
            "trusted" => Some(Self::Trusted),
            _ => None,
        }
    }
}

/// La concesion de un digest dentro de un repositorio, por una decision.
///
/// La concesion **no duplica** a la decision: la nombra con su
/// `decision_id` y conserva solo los cinco datos que hacen falta para
/// juzgar su vigencia hoy. Todo lo demas —los motivos, los requisitos, el
/// desenlace— se lee de la decision persistida.
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
    /// El sujeto (HRN) que la decision evaluo, si la concesion lleva
    /// vigencia. Una concesion anterior a este contrato no lo lleva.
    #[serde(default)]
    pub subject: Option<String>,
    /// La version de la politica con la que se decidio, si la concesion
    /// lleva vigencia.
    #[serde(default)]
    pub policy_version: Option<String>,
    /// El hash de la capsula de evidencia consultada, si la decision la
    /// tuvo. Identifica la evidencia sin duplicarla: el detalle vive en la
    /// decision persistida.
    #[serde(default)]
    pub capsule_hash: Option<String>,
    /// La version del registro de riesgo que se consulto, si se consulto
    /// con vigencia. Cero es un valor honesto: el registro de nadie.
    #[serde(default)]
    pub risk_version: Option<u64>,
    /// Hasta cuando es digna de fe esta concesion, si la lleva. Sin
    /// vigencia declarada no hay concesion activa: es el lado cerrado de
    /// la migracion.
    #[serde(default)]
    pub valid_until: Option<DateTime<Utc>>,
}

impl TrustGrant {
    /// Si la concesion sigue en vigor **a las `now`**.
    ///
    /// Tres condiciones y ninguna caridad: no estar revocada, llevar la
    /// vigencia completa (la version de la politica que decidio) y no
    /// haber caducado. Una concesion sin vigencia —escrita antes de este
    /// contrato o a medias— no es activa: sin prueba de vigencia no se
    /// sirve, y revalidarla es promocionar de nuevo.
    pub fn is_active_at(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none()
            && self.policy_version.is_some()
            && self.valid_until.is_some_and(|until| until > now)
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

    /// Revoca todas las concesiones de una decision en toda la cuenta,
    /// sin saber en que repositorio viven.
    ///
    /// Es la revocacion que pide la reevaluacion: la verdad del riesgo
    /// cambio y la decision que concedio ya no vale, pero quien reevalua
    /// nombra al sujeto y a su cuenta, no a los repositorios donde aquella
    /// decision promovio. Devuelve cuantas estaban activas.
    async fn revoke_decision_for_account(
        &self,
        account: &str,
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

    /// Una concesion activa de ese digest en ese repositorio **a las
    /// `at`**, si la hay.
    ///
    /// La hora es un parametro, no una trampa interna: quien pregunta
    /// decide cual es el ahora, y asi el reloj se puede inyectar en una
    /// prueba. El libro sin filtros responde la vigencia como la guarda.
    async fn active_grant(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        at: DateTime<Utc>,
    ) -> Result<Option<TrustGrant>, CatalogError>;

    /// Todas las concesiones de un repositorio, activas o revocadas.
    async fn grants_of(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustGrant>, CatalogError>;
}

/// A concession the validity sweep looks at: the row, with what the
/// renewal needs and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantSweepRow {
    /// The decision that granted it.
    pub decision_id: String,
    /// The account it belongs to.
    pub account: String,
    /// The repository it covers.
    pub repository: String,
    /// The algorithm of the digest it covers, as stored (`sha256`).
    pub digest_algorithm: String,
    /// The value of the digest it covers.
    pub digest: String,
    /// The subject the decision evaluated, when the grant carries the
    /// validity contract.
    pub subject: Option<String>,
    /// The version of the policy that decided it.
    pub policy_version: Option<String>,
    /// The instant it stops being valid.
    pub valid_until: Option<DateTime<Utc>>,
}

/// The question the validity sweep asks of the book: which concessions
/// are not revoked yet, across every account.
///
/// It is not part of [`TrustLedger`] because only the sweep needs it and
/// the serving path must not grow a way to enumerate the book.
#[async_trait]
pub trait GrantSweepSource: Send + Sync {
    /// The concessions nobody revoked, ordered by account, repository and
    /// digest, at most `limit` of them. The bound keeps a pass finite: the
    /// rest belongs to the next one.
    async fn unrevoked_grants(&self, limit: usize) -> Result<Vec<GrantSweepRow>, CatalogError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DigestScope;
    use chrono::Duration;
    use serde_json::json;

    fn a_grant(now: DateTime<Utc>) -> TrustGrant {
        TrustGrant {
            account: "acme".to_string(),
            repository: "raw".to_string(),
            digest: ContentDigest::of_bytes(b"the bytes"),
            decision_id: "dec-1".to_string(),
            actor: "promoter".to_string(),
            granted_at: now,
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: Some("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app".to_string()),
            policy_version: Some("1.0.0".to_string()),
            capsule_hash: Some("sha256:cap".to_string()),
            risk_version: Some(7),
            valid_until: Some(now + Duration::hours(1)),
        }
    }

    #[test]
    fn a_grant_with_full_validity_is_active_until_it_expires() {
        let now = Utc::now();
        let grant = a_grant(now);
        assert!(grant.is_active_at(now));
        assert!(
            grant.is_active_at(now + Duration::minutes(59)),
            "inside the horizon the promise still holds"
        );
        assert!(
            !grant.is_active_at(now + Duration::hours(1)),
            "valid_until is exclusive: at the deadline the grant is over"
        );
        assert!(!grant.is_active_at(now + Duration::hours(2)));
    }

    #[test]
    fn a_revocation_ends_the_validity_it_has() {
        let now = Utc::now();
        let mut grant = a_grant(now);
        grant.revoked_at = Some(now);
        assert!(!grant.is_active_at(now));
    }

    /// AC de migracion: la vigencia no se presume. Sin version de politica
    /// o sin `valid_until` no hay concesion activa, por muy limpia que
    /// este el resto de la fila.
    #[test]
    fn a_grant_without_declared_validity_is_never_active() {
        let now = Utc::now();
        let mut no_version = a_grant(now);
        no_version.policy_version = None;
        assert!(!no_version.is_active_at(now));

        let mut no_horizon = a_grant(now);
        no_horizon.valid_until = None;
        assert!(!no_horizon.is_active_at(now));

        let mut legacy = a_grant(now);
        legacy.subject = None;
        legacy.capsule_hash = None;
        legacy.risk_version = None;
        legacy.policy_version = None;
        legacy.valid_until = None;
        assert!(
            !legacy.is_active_at(now),
            "a row from before the contract stays out of trusted until revalidated"
        );
    }

    #[test]
    fn the_validity_fields_are_optional_on_the_wire() {
        let mut legacy = serde_json::to_value(a_grant(Utc::now())).expect("a grant serializes");
        let fields = legacy.as_object_mut().expect("a grant is an object");
        for name in [
            "subject",
            "policy_version",
            "capsule_hash",
            "risk_version",
            "valid_until",
        ] {
            fields.remove(name);
        }
        let grant: TrustGrant = serde_json::from_value(legacy).expect("it parses");
        assert!(grant.policy_version.is_none());
        assert!(grant.valid_until.is_none());
        assert!(!grant.is_active_at(Utc::now()));
    }

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
        assert_eq!(ServeChannel::of(&trusted), ServeChannel::Trusted);
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
                ServeChannel::Trusted,
                "{settings}"
            );
        }
    }

    #[test]
    fn a_channel_is_read_from_its_own_name() {
        assert_eq!(ServeChannel::parse("open"), Some(ServeChannel::Open));
        assert_eq!(ServeChannel::parse("trusted"), Some(ServeChannel::Trusted));
        for bad in ["", "OPEN", "everybody", "open "] {
            assert_eq!(ServeChannel::parse(bad), None, "{bad:?} is not a channel");
        }
    }

    #[test]
    fn a_channel_goes_on_the_wire_with_the_name_it_is_read_with() {
        assert_eq!(
            serde_json::to_string(&ServeChannel::Open).expect("json"),
            "\"open\""
        );
        assert_eq!(
            serde_json::to_string(&ServeChannel::Trusted).expect("json"),
            "\"trusted\""
        );
    }
}
