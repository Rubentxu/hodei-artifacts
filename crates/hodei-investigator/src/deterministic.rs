//! La investigacion determinista: la que no pregunta a un modelo.
//!
//! El modelo es **opcional** en el investigador, y cuando no esta, la
//! investigacion no se degrada en una respuesta vacia: es la respuesta de
//! las **herramientas**. Lo que las herramientas calcularon son los hechos,
//! con su cita y con su sello; y lo que no se sabe se **declara** como
//! incertidumbre, porque una seguridad inventada es una invitacion a no
//! comprobar nada.
//!
//! # Lo que esta investigacion NO hace
//!
//! - **No interpreta** la evidencia. El texto de un tercero es dato, y
//!   aqui no hay nadie que lo interprete: lo que se entrega es lo que las
//!   herramientas calcularon, y el texto de la evidencia se queda en el
//!   contexto que se leyo, no en las afirmaciones.
//! - **No responde** la pregunta. Sin modelo, la pregunta se declara sin
//!   responder, y lo que se entrega son los hechos y las incertidumbres.
//! - **No decide** nada. El informe describe; las decisiones las sigue
//!   tomando el motor de politicas determinista.

use hodei_evidence::domain::graph::NodeKind;

use crate::claim::{AIClaim, ClaimKind};
use crate::scope::{FactStatus, ScopedFact};

/// La version de la plantilla de una investigacion sin modelo.
///
/// No hay prompt, y la traza lo dice: una investigacion sin modelo no se
/// puede comparar con una que lo tenga, y una traza que finge un prompt
/// que no existio miente.
pub const NO_PROMPT_VERSION: &str = "none";

/// Quien produce las afirmaciones de una investigacion determinista.
///
/// No es un modelo: son las herramientas, y el informe lo dice.
pub const DETERMINISTIC_PROVIDER: &str = "deterministic";

/// El nombre del "modelo" de una investigacion determinista.
///
/// No hay modelo, y el campo lo dice en vez de inventar uno.
pub const NO_MODEL: &str = "none";

/// Las afirmaciones que las herramientas calculan.
///
/// Un hecho es una **relacion que el mundo tiene**: el enunciado la nombra
/// con los dos HRN y la palabra de relacion, y el verificador determinista
/// la contrasta con los sellos del read-set. Un hecho que el verificador
/// no puede comprobar NO es un hecho: es una afirmacion con cita, y se
/// declara como tal.
///
/// El texto de la evidencia NO se publica como afirmacion: lo escribio un
/// tercero, y aqui no hay nadie que lo interprete. Es contexto, y el
/// manifiesto de lo que se leyo lo dice.
///
/// Y lo que no se pudo leer se declara, con el motivo: una evidencia
/// ausente, un almacen que no contesto, un texto que no se pudo decodificar
/// y un presupuesto que dejo algo fuera son cuatro cosas distintas, y
/// ninguna se disfraza de la otra.
pub fn the_computed_claims(facts: &[ScopedFact]) -> Vec<AIClaim> {
    let mut claims = Vec::new();
    for fact in facts {
        match fact.status {
            // El CONTEXTO leido. El texto es de un tercero: es lo que se
            // leyo, no lo que se afirma, y no se publica como afirmacion.
            //
            // La EXCEPCION es la relacion del grafo: un hecho de tipo
            // Artifact es una relacion que el mundo tiene, y su enunciado
            // lo dice con sus dos HRN y su palabra de relacion.
            FactStatus::Loaded => {
                if fact.kind == NodeKind::Artifact {
                    claims.push(AIClaim::new(
                        fact.text.clone(),
                        ClaimKind::Fact,
                        vec![fact.evidence_id.clone()],
                    ));
                }
            }
            // Lo que no se pudo leer, con el motivo.
            FactStatus::Missing => {
                claims.push(the_uncertainty(format!(
                    "the content of the evidence {} is not there",
                    fact.evidence_id
                )));
            }
            FactStatus::Unavailable => {
                claims.push(the_uncertainty(format!(
                    "the content of the evidence {} could not be read: the store did not answer",
                    fact.evidence_id
                )));
            }
            FactStatus::Undecodable => {
                claims.push(the_uncertainty(format!(
                    "the content of the evidence {} is not text",
                    fact.evidence_id
                )));
            }
            FactStatus::OmittedByBudget => {
                claims.push(the_uncertainty(format!(
                    "the {} of the evidence {} was left out by the budget",
                    fact.kind, fact.evidence_id
                )));
            }
        }
    }
    claims
}

/// La incertidumbre que declara los limites de una respuesta sin modelo.
///
/// Sin modelo no hay quien interprete la evidencia ni quien responda la
/// pregunta, y eso se dice: lo que se entrega son los hechos que las
/// herramientas calcularon, y no una explicacion de mas.
pub fn the_uncertainty_of_a_model_less_answer(question: &str) -> AIClaim {
    let question = question.trim();
    if question.is_empty() {
        return the_uncertainty(
            "there is no model in this deployment: what follows is what the tools computed, and \
             nothing is inferred from it"
                .to_string(),
        );
    }
    the_uncertainty(format!(
        "there is no model in this deployment, so the question is not answered: what follows is \
         what the tools computed about the subject, and nothing is inferred from it. The \
         question was: {question}"
    ))
}

/// Una incertidumbre: lo que no se sabe, dicho como no se sabe.
///
/// Una incertidumbre no cita nada: no afirma nada, y por eso no necesita
/// evidencia que la sostenga.
fn the_uncertainty(statement: String) -> AIClaim {
    AIClaim::new(statement, ClaimKind::Unknown, Vec::new())
}
