//! El hub de artefactos de confianza: la cuarentena, el canal de
//! confianza y la promocion gobernada.
//!
//! La regla del hito es una sola y es tajante: **sin decision de confianza
//! no hay promocion**, y **promocionar no muta los bytes**. El digest es
//! la identidad del contenido, y la promocion cambia el **canal** en el que
//! vive, no lo que hay dentro.
//!
//! Y hay un detalle que parece pequeno y no lo es: un artefacto **recien
//! admitido esta en cuarentena**, siempre. La cuarentena no es un estado
//! exceptional: es el estado por defecto, porque un artefacto sin decision
//! no ha dicho nada todavia.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use hodei_trust_policy::decision::Outcome;
use serde::{Deserialize, Serialize};

use crate::blob::BlobStore;

/// Donde esta un artefacto.
///
/// Los estados son los que el ADR-0015 declara, y son **verificables**: un
/// artefacto del que Hodei solo tiene la referencia no es lo mismo que
/// uno que Hodei tiene y ha promovido.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Solo hay una referencia: Hodei no tiene los bytes.
    ReferenceOnly,
    /// Hodei tiene los bytes, en cuarentena.
    Quarantined,
    /// Hodei tiene los bytes y estan en el canal de confianza.
    Trusted,
    /// Hodei no tiene los bytes.
    Unavailable,
}

/// Un artefacto del hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubArtifact {
    /// El digest: la identidad del contenido.
    pub digest: ContentDigest,
    /// Donde esta.
    pub availability: Availability,
    /// La referencia externa, si la tiene.
    ///
    /// Un artefacto que Hodei no posee puede estar referenciado, y esa
    /// referencia es real sin que Hodei tenga que migrar nada.
    pub external_reference: Option<String>,
}

/// La decision de confianza que gobierna una promocion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustVerdict {
    /// El identificador de la decision.
    pub decision_id: String,
    /// Lo que se decidio.
    pub outcome: Outcome,
    /// Los motivos, con codigos.
    pub reasons: Vec<String>,
    /// La version de la politica que decidio.
    pub policy_version: String,
}

impl TrustVerdict {
    /// Una decision que permite.
    pub fn allowing(decision_id: impl Into<String>, policy_version: impl Into<String>) -> Self {
        Self {
            decision_id: decision_id.into(),
            outcome: Outcome::Allow,
            reasons: Vec::new(),
            policy_version: policy_version.into(),
        }
    }

    /// Una decision que deniega, con sus motivos.
    pub fn denying(
        decision_id: impl Into<String>,
        policy_version: impl Into<String>,
        reasons: Vec<String>,
    ) -> Self {
        Self {
            decision_id: decision_id.into(),
            outcome: Outcome::Deny,
            reasons,
            policy_version: policy_version.into(),
        }
    }
}

/// Por que no se pudo promover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromotionError {
    /// No hay decision: sin decision no hay promocion.
    NoDecision,
    /// La decision deniega.
    Denied {
        /// El identificador de la decision.
        decision_id: String,
        /// Los motivos.
        reasons: Vec<String>,
    },
    /// El artefacto no esta en el hub.
    NotInTheHub,
    /// El almacen no esta disponible.
    Unavailable(String),
}

impl std::fmt::Display for PromotionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDecision => {
                write!(
                    formatter,
                    "there is no trust decision: without one there is no promotion"
                )
            }
            Self::Denied {
                decision_id,
                reasons,
            } => write!(
                formatter,
                "the decision {decision_id} denies the promotion: {}",
                reasons.join(", ")
            ),
            Self::NotInTheHub => write!(formatter, "the artifact is not in the hub"),
            Self::Unavailable(reason) => {
                write!(formatter, "the store is not available: {reason}")
            }
        }
    }
}

/// La traza de una promocion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionAudit {
    /// El digest promovido.
    pub digest: ContentDigest,
    /// La decision que lo permitting.
    pub decision_id: String,
    /// La version de la politica.
    pub policy_version: String,
    /// Quien lo promovio.
    pub by: String,
    /// Cuando.
    pub at: DateTime<Utc>,
    /// Si se denego, los motivos.
    pub reasons: Vec<String>,
}

/// El hub de artefactos de confianza.
pub struct ArtifactHub {
    store: Arc<dyn BlobStore>,
    artifacts: std::sync::Mutex<std::collections::BTreeMap<String, HubArtifact>>,
    audits: std::sync::Mutex<Vec<PromotionAudit>>,
}

impl ArtifactHub {
    /// El hub sobre un almacen de contenido.
    pub fn new(store: Arc<dyn BlobStore>) -> Self {
        Self {
            store,
            artifacts: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            audits: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Admite un artefacto y lo deja **en cuarentena**.
    ///
    /// El artefacto tiene los bytes, y no esta en el canal de confianza:
    /// hasta que haya una decision, no sale de ahi.
    pub async fn ingest(&self, bytes: &[u8]) -> Result<HubArtifact, PromotionError> {
        let digest = self
            .store
            .put(bytes)
            .await
            .map_err(|error| PromotionError::Unavailable(error.to_string()))?;
        let artifact = HubArtifact {
            digest: digest.clone(),
            availability: Availability::Quarantined,
            external_reference: None,
        };
        self.artifacts
            .lock()
            .expect("el hub no esta envenenado")
            .insert(digest.value.clone(), artifact.clone());
        Ok(artifact)
    }

    /// Registra un artefacto **referenciado** pero no poseido.
    ///
    /// Es la coexistencia con los registros externos: Hodei sabe del
    /// artefacto, lo cita y lo referencia, sin migrar sus bytes ni
    /// afirmar que los tiene.
    pub fn reference(
        &self,
        digest: ContentDigest,
        external_reference: impl Into<String>,
    ) -> HubArtifact {
        let artifact = HubArtifact {
            digest: digest.clone(),
            availability: Availability::ReferenceOnly,
            external_reference: Some(external_reference.into()),
        };
        self.artifacts
            .lock()
            .expect("el hub no esta envenenado")
            .insert(digest.value.clone(), artifact.clone());
        artifact
    }

    /// Promueve un artefacto al canal de confianza.
    ///
    /// La promocion **no copia y no muta los bytes**: lo que cambia es el
    /// canal, y el canal de confianza referencia **el digest exacto**.
    pub async fn promote(
        &self,
        digest: &ContentDigest,
        verdict: Option<&TrustVerdict>,
        by: &str,
        at: DateTime<Utc>,
    ) -> Result<HubArtifact, PromotionError> {
        let mut artifacts = self.artifacts.lock().expect("el hub no esta envenenado");
        let Some(artifact) = artifacts.get_mut(&digest.value.clone()) else {
            return Err(PromotionError::NotInTheHub);
        };

        // Sin decision, no hay promocion.
        let Some(verdict) = verdict else {
            let audit = PromotionAudit {
                digest: digest.clone(),
                decision_id: String::new(),
                policy_version: String::new(),
                by: by.to_string(),
                at,
                reasons: vec!["there is no trust decision".to_string()],
            };
            self.audits
                .lock()
                .expect("el hub no esta envenenado")
                .push(audit);
            return Err(PromotionError::NoDecision);
        };

        if verdict.outcome != Outcome::Allow {
            let audit = PromotionAudit {
                digest: digest.clone(),
                decision_id: verdict.decision_id.clone(),
                policy_version: verdict.policy_version.clone(),
                by: by.to_string(),
                at,
                reasons: verdict.reasons.clone(),
            };
            self.audits
                .lock()
                .expect("el hub no esta envenenado")
                .push(audit);
            return Err(PromotionError::Denied {
                decision_id: verdict.decision_id.clone(),
                reasons: verdict.reasons.clone(),
            });
        }

        // El digest NO cambia: la promocion cambia el canal.
        artifact.availability = Availability::Trusted;
        let promoted = artifact.clone();

        self.audits
            .lock()
            .expect("el hub no esta envenenado")
            .push(PromotionAudit {
                digest: digest.clone(),
                decision_id: verdict.decision_id.clone(),
                policy_version: verdict.policy_version.clone(),
                by: by.to_string(),
                at,
                reasons: verdict.reasons.clone(),
            });

        Ok(promoted)
    }

    /// Los bytes de un artefacto, verificados por su digest.
    pub async fn bytes_of(&self, digest: &ContentDigest) -> Result<Vec<u8>, PromotionError> {
        self.store
            .get(digest)
            .await
            .map_err(|error| PromotionError::Unavailable(error.to_string()))
    }

    /// Un artefacto del hub.
    pub fn artifact(&self, digest: &ContentDigest) -> Option<HubArtifact> {
        self.artifacts
            .lock()
            .expect("el hub no esta envenenado")
            .get(&digest.value.clone())
            .cloned()
    }

    /// El canal de confianza: lo que se puede publicar.
    pub fn trusted(&self) -> Vec<HubArtifact> {
        self.artifacts
            .lock()
            .expect("el hub no esta envenenado")
            .values()
            .filter(|artifact| artifact.availability == Availability::Trusted)
            .cloned()
            .collect()
    }

    /// La cuarentena: lo que no se puede publicar todavia.
    pub fn quarantine(&self) -> Vec<HubArtifact> {
        self.artifacts
            .lock()
            .expect("el hub no esta envenenado")
            .values()
            .filter(|artifact| artifact.availability == Availability::Quarantined)
            .cloned()
            .collect()
    }

    /// La traza de las promociones.
    pub fn audits(&self) -> Vec<PromotionAudit> {
        self.audits
            .lock()
            .expect("el hub no esta envenenado")
            .clone()
    }
}
