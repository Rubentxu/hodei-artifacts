//! El read-set del investigador.
//!
//! La primitiva vive en [`hodei_evidence::domain::read_set`] porque es
//! de DOMINIO compartido: el investigador la usa para sellar lo que leyo
//! y la simulacion la usa para fijar la base sobre la que evalua. Este
//! modulo es la puerta de entrada del investigador y no define nada:
//! reexporta la misma primitiva, de modo que un manifiesto escrito por
//! uno verifica el del otro.

pub use hodei_evidence::domain::read_set::{ReadKind, ReadSet, ReadStamp};
