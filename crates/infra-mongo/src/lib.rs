//! Infraestructura MongoDB compartida.
//!
//! Objetivo (INFRA-T1):
//! - Centralizar carga de configuración (env → MongoConfig).
//! - Proveer inicialización perezosa (lazy) y segura del `mongodb::Client`.
//! - Exponer un handle minimalista para que los adapters (persistence) de otros crates
//!   obtengan `mongodb::Database` o `Collection<T>` sin acoplarse a detalles.
//!
//! Principios aplicados (ver docs/arquitectura-sistema.md):
//! - Hexagonal: este crate solo ofrece infraestructura (no lógica de dominio).
//! - Vertical Slice Friendly: otros bounded contexts lo usan para implementar sus adapters.
//! - No añade dependencias hacia dominios; únicamente tipos infra.
//!
//! Uso típico en un adapter (ejemplo conceptual):
//! ```rust,ignore
//! use infra_mongo::{MongoClientFactory, MongoConfig};
//!
//! async fn init_repo() -> anyhow::Result<()> {
//!     let cfg = MongoConfig::from_env()?;
//!     let factory = MongoClientFactory::new(cfg);
//!     let db = factory.database().await?; // mongodb::Database
//!     let coll = db.inner().collection<bson::Document>("artifacts");
//!     // ...
//!     Ok(())
//! }
//! ```
//!
//! Módulos:
//! - config: carga y representación de `MongoConfig`.
//! - error: errores de infraestructura.
//! - client: factoría perezosa y handle de base de datos.
//! - test_util (feature `test-util`): utilidades de pruebas de integración.

pub mod config;
pub mod error;
pub mod client;

pub use config::MongoConfig;
pub use client::{MongoClientFactory, MongoDatabaseHandle};
pub use error::MongoInfraError;

#[cfg(feature = "test-util")]
pub mod test_util;
