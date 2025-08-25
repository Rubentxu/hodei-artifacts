use hodei_artifacts_api::{bootstrap, Application};
use std::net::TcpListener;
use tokio::sync::Mutex;
use std::sync::Arc;

/// Inicia una instancia del servidor en un puerto aleatorio para los tests de integración.
///
/// Configura las variables de entorno necesarias para que el servidor se conecte
/// a los servicios de test (contenedores Docker).
///
/// # Arguments
///
/// * `mongo_port` - El puerto en el que el contenedor de MongoDB está escuchando.
/// * `s3_port` - El puerto en el que el contenedor de MinIO (S3) está escuchando.
/// * `kafka_port` - El puerto en el que el contenedor de Kafka está escuchando.
///
/// # Returns
///
/// El puerto en el que el servidor de la aplicación está escuchando.
pub async fn start_server(mongo_port: u16, s3_port: u16, kafka_port: u16) -> u16 {
    // Establecer variables de entorno para la configuración de la aplicación
    unsafe {
        std::env::set_var("MONGO_URI", format!("mongodb://localhost:{}", mongo_port));
        std::env::set_var("MONGO_DATABASE", "hodei-test");
        std::env::set_var("S3_ENDPOINT", format!("http://localhost:{}", s3_port));
        std::env::set_var("S3_BUCKET_NAME", "hodei-artifacts");
        std::env::set_var("S3_REGION", "us-east-1");
        std::env::set_var("S3_ACCESS_KEY", "minioadmin");
        std::env::set_var("S3_SECRET_KEY", "minioadmin");
        std::env::set_var("KAFKA_BROKERS", format!("localhost:{}", kafka_port));
    }

    // Vincular a un puerto aleatorio disponible
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    let port = listener.local_addr().unwrap().port();

    // Bootstrap de la aplicación
    let app_state = bootstrap().await.expect("Failed to bootstrap application state");
    let application = Application::new(port, app_state).await;

    // Ejecutar la aplicación en un hilo separado
    tokio::spawn(async move {
        application.run().await.expect("Failed to run application");
    });

    port
}
