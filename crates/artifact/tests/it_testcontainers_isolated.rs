#![cfg(feature = "integration")]

#[cfg(test)]
mod tests {
    use testcontainers::{core::WaitFor, runners::AsyncRunner, GenericImage};
    use tracing::info;

    #[tokio::test]
    async fn test_hello_world_container() {
        info!("Starting hello-world container test");

        let image = GenericImage::new("hello-world", "latest")
            .with_wait_for(WaitFor::message_on_stdout("Hello from Docker!"));

        let container = image.start().await;

        assert!(container.is_ok(), "Failed to start hello-world container");

        if let Ok(container) = container {
            info!("hello-world container started successfully");
            info!("Container ID: {}", container.id());
            // The container will be stopped and removed automatically when it goes out of scope.
        }
        
        info!("hello-world container test completed successfully");
    }
}
