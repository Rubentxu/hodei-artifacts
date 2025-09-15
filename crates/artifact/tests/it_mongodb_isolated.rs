#![cfg(feature = "integration")]

#[cfg(test)]
mod tests {
    use mongodb::{bson::doc, Client};
    use std::time::Duration;
    use testcontainers::{core::WaitFor, runners::AsyncRunner, GenericImage, ImageExt};
    use tracing::info;

    #[tokio::test]
    async fn test_mongodb_isolated_connection() -> Result<(), mongodb::error::Error> {
        info!("Starting MongoDB container for isolated test");

        // Define the MongoDB image
        let image =
            GenericImage::new("mongo", "5.0").with_startup_timeout(Duration::from_secs(180));

        // Start the MongoDB container
        let container = image.start().await.unwrap();

        // Get the dynamic host port
        let host_port = container.get_host_port_ipv4(27017).await.unwrap();

        // Construct the connection string
        let uri = format!("mongodb://127.0.0.1:{}", host_port);
        info!("Connecting to MongoDB at: {}", uri);

        // Connect a client to the database
        let client = Client::with_uri_str(&uri).await?;

        // Ping the database to verify the connection
        client
            .database("admin")
            .run_command(doc! {"ping": 1}, None)
            .await?;
        info!("Successfully pinged MongoDB");

        // Perform a database operation
        let db = client.database("test-db");
        let collection = db.collection("test-collection");

        info!("Inserting document into collection");
        let insert_result = collection.insert_one(doc! { "x": 42 }, None).await?;
        assert!(insert_result.inserted_id.as_object_id().is_some());
        info!("Document inserted successfully");

        info!("Querying for inserted document");
        let find_result = collection
            .find_one(doc! { "x": 42 }, None)
            .await?
            .expect("Failed to find document");

        assert_eq!(find_result.get_i32("x").unwrap(), 42);
        info!("Successfully found and verified document");

        // The container is automatically stopped and removed when it goes out of scope.
        Ok(())
    }
}
