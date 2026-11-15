//! El adaptador del catálogo sobre SurrealDB.
//!
//! El dominio vive en `hodei-catalog`, que no sabe que existe una base de
//! datos. Aquí es donde se sabe: las tablas del catálogo, los índices que
//! decidan los conflictos, y las transacciones que hacen que publicar sea
//! una sola cosa y no cuatro.
//!
//! La división no es tidiness. Es una garantía que el compilador
//! comprueba: el crate del dominio **no puede** nombrar un motor de base
//! de datos, así que sus contratos no pueden depender de uno por accidente.

#![forbid(unsafe_code)]

pub mod surreal;

pub use surreal::{SCHEMA_VERSION, SurrealCatalogStore};
