//! El adaptador del catalogo sobre SurrealDB.
//!
//! El dominio vive en `hodei-catalog`, que no sabe que existe una base de
//! datos. Aqui es donde se sabe: las tablas del catalogo, los indices que
//! decidan los conflictos, y las transacciones que hacen que publicar sea
//! una sola cosa y no cuatro.
//!
//! La division no es tidiness. Es una garantia que el compilador
//! comprueba: el crate del dominio **no puede** nombrar un motor de base
//! de datos, asi que sus contratos no pueden depender de uno por accidente.

#![forbid(unsafe_code)]

pub mod surreal;

pub use surreal::{SCHEMA_VERSION, SurrealCatalogStore};
