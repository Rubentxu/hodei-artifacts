use tokio::signal;
use tracing::{info, warn};

pub async fn signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal, initiating graceful shutdown");
        },
        _ = terminate => {
            info!("Received SIGTERM signal, initiating graceful shutdown");
        },
    }
    
    // Give some time for cleanup
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    info!("Shutdown signal processed");
}

pub async fn graceful_shutdown(timeout: std::time::Duration) {
    info!("Starting graceful shutdown with {}s timeout", timeout.as_secs());
    
    // Wait for the shutdown signal
    signal().await;
    
    // Additional cleanup tasks can be added here
    // For example: flushing metrics, closing database connections, etc.
    
    // Wait a bit to ensure all ongoing requests complete
    let cleanup_time = std::cmp::min(timeout, std::time::Duration::from_secs(5));
    tokio::time::sleep(cleanup_time).await;
    
    info!("Graceful shutdown completed");
}
