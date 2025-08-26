//! Contenedor de utilidades para levantar una instancia de MongoDB para tests de integración.
//! Sigue la estrategia definida en `docs/test-containers.md`.

use crate::client::MongoClientFactory;
use crate::config::MongoConfig;
use anyhow::Result;
use testcontainers_modules::mongo::Mongo;
use testcontainers::{runners::AsyncRunner, ContainerAsync};
use tracing::info;
use tokio::time::{sleep, Duration}; // Add this import

// Contenedor Docker que se mantiene vivo durante toda la ejecución de los tests.
pub struct TestMongoContainer {
    _container: ContainerAsync<Mongo>,
}

/// Levanta una instancia de MongoDB en un contenedor efímero si no se proveen
/// variables de entorno `MONGO_*`.
///
/// Retorna una factoría de cliente configurada y, opcionalmente, el guardián del
/// contenedor para mantenerlo vivo.
pub async fn ephemeral_store() -> Result<(MongoClientFactory, Option<TestMongoContainer>)> {
    // Intenta cargar la configuración desde variables de entorno.
    if std::env::var("MONGO_URI").is_ok() {
        info!("Usando base de datos MongoDB externa desde variables de entorno");
        let config = MongoConfig::from_env()
            .map_err(|e| anyhow::anyhow!("Error al cargar la configuración de MongoDB desde el entorno: {}", e))?;
        let factory = MongoClientFactory::new(config);
        return Ok((factory, None));
    }

    info!("No se encontraron variables de entorno MONGO_*, levantando contenedor efímero...");
    let container = Mongo::default().start().await?;
    let port = container.get_host_port_ipv4(27017).await?;
    let db_name = "hodei-test-db";

    let config = MongoConfig {
        uri: format!("mongodb://localhost:{}", port),
        database: db_name.to_string(),
        min_pool_size: None,
        max_pool_size: None,
        app_name: Some("hodei-test-runner".to_string()),
        tls: None,
    };

    let factory = MongoClientFactory::new(config);

    // --- ADD RETRY LOGIC HERE ---
    let max_retries = 10;
    for i in 0..max_retries {
        match factory.client().await {
            Ok(_) => {
                info!("Conexión a MongoDB establecida después de {} intentos.", i + 1);
                break; // Connection successful, break the loop
            }
            Err(e) => {
                if i == max_retries - 1 {
                    return Err(anyhow::anyhow!("Fallo al conectar a MongoDB después de {} intentos: {}", max_retries, e));
                }
                info!("Intento {} de {}: Fallo al conectar a MongoDB: {}. Reintentando...", i + 1, max_retries, e);
                sleep(Duration::from_millis(500)).await;
            }
        }
    }
    // --- END RETRY LOGIC ---

    let test_container = TestMongoContainer {
        _container: container,
    };

    info!(
        "Contenedor MongoDB disponible en el puerto anfitrión {} y base de datos '{}'",
        port, db_name
    );

    Ok((factory, Some(test_container)))
}
