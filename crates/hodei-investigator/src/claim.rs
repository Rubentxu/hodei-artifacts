//! La afirmacion de la investigacion: lo que dice el modelo, con su
//! procedencia.
//!
//! Un AIClaim no es texto: es una afirmacion **tipada** que dice de que
//! tipo es, **cita** la evidencia de la que sale y lleva la traza de quien
//! la produjo. Y una afirmacion sin cita resoluble no es una afirmacion:
//! es una fabricacion, y no se publica.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// De que tipo es la afirmacion.
///
/// La distincion entre un **hecho** y una **inferencia** es la que hace
/// que la explicacion sea auditable: el hecho lo han calculado las
/// herramientas deterministas, y la inferencia la ha hecho el modelo. Una
/// explicacion que no los distingue es una explicacion en la que no se
/// puede apoyar nada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    /// Un hecho calculado por las herramientas deterministas.
    Fact,
    /// Una inferencia del modelo a partir de los hechos.
    Inference,
    /// Algo que no se sabe.
    ///
    /// El modelo puede decir "no lo se", y es lo HONRATO: una
    /// incertidumbre declarada es una invitation a comprobar, y una
    /// seguridad inventada es una invitation a no comprobar nada.
    Unknown,
    /// Una recomendacion.
    Recommendation,
}

/// Una afirmacion con su procedencia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AIClaim {
    /// Lo que se afirma.
    pub statement: String,
    /// De que tipo es.
    pub kind: ClaimKind,
    /// Las citas: los identificadores de la evidencia de la que sale.
    pub citations: Vec<String>,
}

impl AIClaim {
    /// Una afirmacion.
    pub fn new(statement: impl Into<String>, kind: ClaimKind, citations: Vec<String>) -> Self {
        Self {
            statement: statement.into(),
            kind,
            citations,
        }
    }
}

/// Por que una afirmacion no vale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimError {
    /// No dice nada.
    Empty,
    /// Un hecho sin cita es una fabricacion.
    AFactWithoutCitations,
    /// Una cita que no resuelve.
    UnresolvedCitation(String),
    /// Una afirmacion que no es del tipo que se pidio.
    NotAllowed(ClaimKind),
}

impl std::fmt::Display for ClaimError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(formatter, "the claim says nothing"),
            Self::AFactWithoutCitations => {
                write!(formatter, "a fact without a citation is a fabrication")
            }
            Self::UnresolvedCitation(citation) => {
                write!(formatter, "the citation {citation} does not resolve")
            }
            Self::NotAllowed(kind) => write!(
                formatter,
                "a claim of kind {kind:?} is not allowed where it is"
            ),
        }
    }
}

/// Comprueba una afirmacion contra la evidencia que se recupero.
///
/// Las reglas son tres y son deliberadas:
///
/// - un **hecho** necesita al menos una cita, y la cita tiene que
///   resolverse: un hecho sin evidencia es una fabricacion;
/// - una **inferencia** puede no citar, pero entonces es una opinion y
///   se declara como tal;
/// - una **incertidumbre** no necesita citas: lo que dice es que no se
///   sabe.
pub fn check_the_claim(
    claim: &AIClaim,
    known_evidence: &[String],
    allowed: &[ClaimKind],
) -> Result<(), ClaimError> {
    if claim.statement.trim().is_empty() {
        return Err(ClaimError::Empty);
    }
    if !allowed.contains(&claim.kind) {
        return Err(ClaimError::NotAllowed(claim.kind));
    }
    for citation in &claim.citations {
        if !known_evidence.contains(citation) {
            return Err(ClaimError::UnresolvedCitation(citation.clone()));
        }
    }
    if claim.kind == ClaimKind::Fact && claim.citations.is_empty() {
        return Err(ClaimError::AFactWithoutCitations);
    }
    Ok(())
}

/// La traza de una investigacion.
///
/// La traza dice **quien** produjo la respuesta, **con que** prompt, **sobre
/// que** contexto y **cuando**. Sin eso, una respuesta no se puede
/// reproducir ni auditar, y una explicacion que no se puede auditar no
/// es una explicacion: es una opinion con formato.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvestigationTrace {
    /// El identificador de la investigacion.
    pub investigation: String,
    /// Quien produjo la respuesta.
    pub provider: String,
    /// El modelo.
    pub model: String,
    /// La version del modelo, si la hay.
    pub model_version: Option<String>,
    /// La version de la plantilla del prompt.
    pub prompt_version: String,
    /// Los identificadores de la evidencia que se le dio.
    pub context_evidence: Vec<String>,
    /// Cuantas palabras teve el contexto.
    pub context_words: usize,
    /// Cuando se hizo.
    pub at: DateTime<Utc>,
}

impl InvestigationTrace {
    /// La traza de una investigacion.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        investigation: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
        model_version: Option<String>,
        prompt_version: impl Into<String>,
        context_evidence: Vec<String>,
        at: DateTime<Utc>,
    ) -> Self {
        Self {
            investigation: investigation.into(),
            provider: provider.into(),
            model: model.into(),
            model_version,
            prompt_version: prompt_version.into(),
            context_words: context_evidence.len(),
            context_evidence,
            at,
        }
    }
}
