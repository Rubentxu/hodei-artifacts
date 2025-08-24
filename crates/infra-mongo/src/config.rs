//! Configuración de MongoDB (ver docs/arquitectura-sistema.md sección Persistencia).
//!
//! Responsabilidades:
//! - Cargar variables de entorno requeridas / opcionales.
//! - Representar una configuración inmutable (`MongoConfig`).
//! - No realiza conexión (Single Responsibility: solo configuración).
//!
//! Variables soportadas:
//! - MONGO_URI (obligatoria)
//! - MONGO_DATABASE (obligatoria)
//! - MONGO_MIN_POOL_SIZE (opcional, u32)
//! - MONGO_MAX_POOL_SIZE (opcional, u32)
//! - MONGO_APP_NAME (opcional, String)
//! - MONGO_TLS (opcional, bool)
use serde::Deserialize;
use std::env;

#[derive(Debug, Clone, Deserialize)]
pub struct MongoConfig {
    pub uri: String,
    pub database: String,
    pub min_pool_size: Option<u32>,
    pub max_pool_size: Option<u32>,
    pub app_name: Option<String>,
    pub tls: Option<bool>,
}

impl MongoConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let get = |k: &str| env::var(k).map_err(|e| anyhow::anyhow!("Falta variable {k}: {e}"));
        Ok(Self {
            uri: get("MONGO_URI")?,
            database: get("MONGO_DATABASE")?,
            min_pool_size: env::var("MONGO_MIN_POOL_SIZE").ok().and_then(|v| v.parse().ok()),
            max_pool_size: env::var("MONGO_MAX_POOL_SIZE").ok().and_then(|v| v.parse().ok()),
            app_name: env::var("MONGO_APP_NAME").ok(),
            tls: env::var("MONGO_TLS").ok().and_then(|v| v.parse().ok()),
        })
    }
}