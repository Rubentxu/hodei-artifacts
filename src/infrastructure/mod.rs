//! Módulo de infraestructura de la aplicación
//!
//! Contiene adaptadores que conectan los bounded contexts entre sí
//! y con el kernel compartido. Esta es la composition root donde se
//! realiza el cableado de dependencias.

pub mod adapters;

pub use adapters::GetEffectiveScpsAdapter;

