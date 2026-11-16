//! La recoleccion de basura del almacen de contenido.
//!
//! - [`plan`]: el nucleo puro. Dado lo que hay, lo que el catalogo
//!   referencia y las retenciones, dice que se conserva, que esta en
//!   gracia y que es reclamable, y por que.
//! - [`service`]: lo une al almacen, al catalogo y al libro, y ofrece el
//!   dry-run y el barrido durable.

pub mod plan;
pub mod service;

pub use plan::{Entry, Plan, Reason, Summary, Verdict};
pub use service::{GcError, GcService, SweepReport};
