//! El adaptador del catálogo sobre Dgraph.
//!
//! El dominio vive en `hodei-catalog`, que no sabe que existe una base de
//! datos. Aquí es donde se sabe: las tablas del catálogo, los índices que
//! decidan los conflictos, y las transacciones que hacen que publicar sea
//! una sola cosa y no cuatro.
//!
//! La división no es tidiness. Es una garantía que el compilador
//! comprueba: el crate del dominio **no puede** nombrar un motor de base de
//! datos, así que sus contratos no pueden depender de uno por accidente.
//!
//! Aquí vive también el **outbox de auditoría**: la tabla donde espera el
//! evento de una publicación, y el adaptador que lo entrega. Vive en esta
//! base y no en otra a propósito, porque la fila del evento se escribe
//! dentro de la misma transacción que publica, y una transacción que toca
//! dos bases no es una transacción.

#![forbid(unsafe_code)]

// El puerto Dgraph del catálogo (HSP-187, slice Raw): el mismo contrato
// de HAR-02/HAR-03 sobre el grafo nativo, con el outbox de auditoría en
// la MISMA transacción que el commit.
pub mod dgraph;

pub use dgraph::{CATALOG_DGRAPH_SCHEMA_VERSION, DgraphAuditOutbox, DgraphCatalogStore};
