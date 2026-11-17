//! La investigacion contrafactual: explicar por que algo esta bloqueado
//! y que cambiaria bajo una hipotesis, **sin ejecutar nada**.
//!
//! El investigador explica el mundo con citas de evidencia. La
//! simulacion dice que pasaria si. Cada uno por su lado es una mitad
//! de la respuesta que pide una release bloqueada; este crate los
//! compone en un unico contrato auditable, y no añade una tercera
//! verdad: el informe contrafactual **es** el informe de investigacion
//! mas el resultado de simulacion, con las etiquetas epistemicas que
//! dicen de donde sale cada enunciado.
//!
//! # Las etiquetas, no la prosa
//!
//! Un enunciado del informe lleva su clase: lo que el verificador
//! determinista verifico (`verified`), lo que se observo con cita
//! resoluble (`observed`), lo que solo vale bajo la hipotesis
//! (`hypothetical`), lo que el modelo infirio (`inference`) y lo que no
//! se sabe (`unknown`). Una recomendacion Jamas se atribuye al motor de
//! politicas: el motor decide con reglas, y lo que aqui se recomienda
//! lo dice quien pregunta, no las reglas.
//!
//! # El sello compartido
//!
//! La investigacion y la simulacion leen el MISMO manifiesto de
//! lecturas --[`hodei_evidence::domain::read_set`]--, y por eso el
//! informe puede comprobar que ambas mitades hablan del mismo mundo:
//! si una fuente aparece en los dos manifiestos con versiones
//! distintas, la respuesta se rechaza como sello incoherente antes que
//! publicarse.
//!
//! # Fallar cerrado, sin filtrar
//!
//! Una hipotesis de otra cuenta, caducada o calculada contra un mundo
//! que ya no existe se rechaza con un motivo que no lleva identificadores
//! ajenos, y el rechazo ocurre ANTES de recuperar nada: no se filtra
//! dato alguno del mundo que la hipotesis decia tocar.

#![forbid(unsafe_code)]

pub mod report;
pub mod service;

pub use report::{
    COUNTERFACTUAL_REPORT_VERSION, CounterfactualReport, EpistemicLabel, SealCoherence, Statement,
    the_seal_coherence,
};
pub use service::{CounterfactualError, CounterfactualService, RejectionReason};
