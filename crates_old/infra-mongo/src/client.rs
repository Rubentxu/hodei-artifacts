//! Cliente y factoría perezosa de MongoDB.
//!
//! Responsabilidades:
//! - Encapsular inicialización del `mongodb::Client` (lazy + thread-safe).
//! - Exponer un handle ligero (`MongoDatabaseHandle`) para uso en adapters de persistencia.
//! - Mantener fuera de los bounded contexts detalles de configuración de opciones.
//!
//! Principios (ver docs/arquitectura-sistema.md):
//! - No filtra tipos concretos innecesarios al resto de crates.
//! - Facilita testeo mediante la feature `test-util` (limpieza / DB aislada).
use crate::{config::MongoConfig, error::MongoInfraError};
use mongodb::{
    options::{ClientOptions, ServerApi, ServerApiVersion},
    Client, Database,
};
use tokio::sync::OnceCell;
use tracing::{debug, info};

/// Handle de acceso a la base de datos; envuelve `mongodb::Database`.
#[derive(Clone)]
pub struct MongoDatabaseHandle {
    database: Database,
}

impl MongoDatabaseHandle {
    pub fn inner(&self) -> &Database {
        &self.database
    }
}

/// Factoría perezosa (thread-safe) para un `mongodb::Client`.
///
/// Diseñada para inyectarse (normalmente envuelta en `Arc`) en adaptadores de persistencia
/// dentro de cada bounded context sin exponer detalles de inicialización.
pub struct MongoClientFactory {
    config: MongoConfig,
    client: OnceCell<Client>,
}

impl MongoClientFactory {
    /// Crea una nueva factoría con la configuración indicada.
    pub fn new(config: MongoConfig) -> Self {
        Self {
            config,
            client: OnceCell::new(),
        }
    }

    /// Devuelve (creando si es necesario) el `Client` compartido.
    async fn get_or_init_client(&self) -> Result<&Client, MongoInfraError> {
        self.client
            .get_or_try_init(|| async {
                let mut opts = ClientOptions::parse(&self.config.uri).await?;

                // Establecer Server API para estabilidad (útil en Atlas / versiones fijas).
                opts.server_api = Some(ServerApi::builder().version(ServerApiVersion::V1).build());

                if let Some(min) = self.config.min_pool_size {
                    opts.min_pool_size = Some(min);
                }
                if let Some(max) = self.config.max_pool_size {
                    opts.max_pool_size = Some(max);
                }
                if let Some(ref name) = self.config.app_name {
                    opts.app_name = Some(name.clone());
                }

                // Nota: configuración TLS custom omitida por ahora (el campo usado anteriormente no existe en ClientOptions 2.8).
                // Si se requiere manipulación futura:
                //   - Usar `opts.tls = Some(Tls::enabled())` y ajustar propiedades del struct `Tls`.
                //   - Añadir feature de configuración condicional siguiendo necesidades reales.

                debug!("Inicializando cliente Mongo (pool)...");
                let client = Client::with_options(opts)?;
                info!("Cliente Mongo inicializado");
                // Anotación explícita para evitar ambigüedad de tipo del Result inferido por el compilador.
                Ok::<Client, mongodb::error::Error>(client)
            })
            .await
            .map_err(MongoInfraError::from)
    }

    /// Retorna un handle al `mongodb::Database`.
    pub async fn database(&self) -> Result<MongoDatabaseHandle, MongoInfraError> {
        let client = self.get_or_init_client().await?;
        Ok(MongoDatabaseHandle {
            database: client.database(&self.config.database),
        })
    }

    /// Retorna una referencia al `mongodb::Client` inicializado.
    pub async fn client(&self) -> Result<&Client, MongoInfraError> {
        self.get_or_init_client().await
    }

    /// Ejecuta un ping para health-check.
    pub async fn ping(&self) -> Result<(), MongoInfraError> {
        let db = self.database().await?;
        db.inner()
            .run_command(mongodb::bson::doc! { "ping": 1 })
            .await?;
        Ok(())
    }
}
