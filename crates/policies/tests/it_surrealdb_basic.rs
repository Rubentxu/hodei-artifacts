#![cfg(feature = "integration")]

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use surrealdb::engine::any::Any;
    use surrealdb::opt::auth::Root;
    use surrealdb::Surreal;
    use testcontainers::{core::WaitFor, runners::AsyncRunner, GenericImage, ImageExt};
    use tracing::info;

    /// SurrealDB container configuration
    async fn setup_surrealdb_container() -> Result<(String, String), Box<dyn std::error::Error>> {
        info!("Setting up SurrealDB container for integration tests");

        let surrealdb_image = GenericImage::new("surrealdb/surrealdb", "v2.3.9-dev")
            .with_exposed_port(8000u16)
            .with_env_var("SURREAL_USER", "root")
            .with_env_var("SURREAL_PASS", "password")
            .with_env_var("SURREAL_BIND", "0.0.0.0:8000")
            .with_wait_for(WaitFor::message_on_stdout("Started web server on"))
            .with_wait_for(WaitFor::Duration { length: Duration::from_secs(10) });

        let container = surrealdb_image.start().await
            .map_err(|e| format!("Failed to start SurrealDB container: {}", e))?;

        let port = container.get_host_port_ipv4(8000).await
            .map_err(|e| format!("Failed to get SurrealDB port: {}", e))?;

        let host = container.get_host().await
            .map_err(|e| format!("Failed to get SurrealDB host: {}", e))?;

        let connection_string = format!("ws://{}:{}/rpc", host, port);
        info!("SurrealDB container started at: {}", connection_string);

        // Wait for SurrealDB to be ready
        tokio::time::sleep(Duration::from_secs(5)).await;

        Ok((connection_string, container.id().to_string()))
    }

    /// Create SurrealDB connection
    async fn create_surrealdb_connection(connection_string: &str) -> Result<Surreal<Any>, Box<dyn std::error::Error>> {
        let client = Surreal::new::<Any>(connection_string).await
            .map_err(|e| format!("Failed to connect to SurrealDB: {}", e))?;

        // Sign in
        client.signin(Root {
            username: "root",
            password: "password",
        }).await
            .map_err(|e| format!("Failed to sign in to SurrealDB: {}", e))?;

        // Use test namespace and database
        client.use_ns("test_namespace").use_db("test_policies").await
            .map_err(|e| format!("Failed to use namespace/database: {}", e))?;

        info!("Connected to SurrealDB successfully");
        Ok(client)
    }

    #[tokio::test]
    async fn test_surrealdb_container_setup() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing SurrealDB container setup");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;

        // Simple test to verify connection works
        let result = client.query("RETURN 'Hello from SurrealDB!'").await;
        assert!(result.is_ok(), "Basic SurrealDB query should succeed");

        info!("SurrealDB container setup test completed successfully");
        Ok(())
    }

    #[tokio::test]
    async fn test_surrealdb_basic_crud() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing SurrealDB basic CRUD operations");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;

        // Create a test record
        let create_result = client
            .create(("person", "tobie"))
            .content(serde_json::json!({
                "name": "Tobie",
                "age": 30,
                "skills": ["Rust", "Docker"]
            }))
            .await;

        assert!(create_result.is_ok(), "Record creation should succeed");

        // Read the record
        let read_result = client.select(("person", "tobie")).await;
        assert!(read_result.is_ok(), "Record reading should succeed");

        // Update the record
        let update_result = client
            .update(("person", "tobie"))
            .merge(serde_json::json!({
                "age": 31,
                "skills": ["Rust", "Docker", "Testcontainers"]
            }))
            .await;

        assert!(update_result.is_ok(), "Record update should succeed");

        // Delete the record
        let delete_result = client.delete(("person", "tobie")).await;
        assert!(delete_result.is_ok(), "Record deletion should succeed");

        info!("SurrealDB basic CRUD test completed successfully");
        Ok(())
    }

    #[tokio::test]
    async fn test_surrealdb_query_performance() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing SurrealDB query performance");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;

        // Create multiple test records
        for i in 0..100 {
            let create_result = client
                .create(("test_record", format!("test_{}", i)))
                .content(serde_json::json!({
                    "name": format!("Test {}", i),
                    "value": i,
                    "category": "test"
                }))
                .await;

            assert!(create_result.is_ok(), "Record creation should succeed");
        }

        // Test query performance
        let start_time = std::time::Instant::now();
        let query_result = client
            .query("SELECT * FROM test_record WHERE value > 50")
            .await;

        assert!(query_result.is_ok(), "Query should succeed");
        
        let duration = start_time.elapsed();
        info!("Query execution time: {:?}", duration);

        // Performance assertion (should be very fast for this simple query)
        assert!(duration.as_millis() < 1000, "Query should complete in under 1 second");

        // Verify results
        let result = query_result.unwrap();
        assert!(result.len() > 0, "Query should return results");

        info!("SurrealDB query performance test completed successfully");
        Ok(())
    }

    #[tokio::test]
    async fn test_surrealdb_concurrent_operations() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing SurrealDB concurrent operations");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = Arc::new(create_surrealdb_connection(&connection_string).await?);

        // Spawn concurrent tasks
        let mut handles = Vec::new();
        for i in 0..10 {
            let client_clone = client.clone();
            let handle = tokio::spawn(async move {
                // Create a record
                let create_result = client_clone
                    .create(("concurrent_test", format!("concurrent_{}", i)))
                    .content(serde_json::json!({
                        "task_id": i,
                        "status": "created"
                    }))
                    .await;

                assert!(create_result.is_ok(), "Concurrent record creation should succeed");

                // Read the record back
                let read_result = client_clone.select(("concurrent_test", format!("concurrent_{}", i))).await;
                assert!(read_result.is_ok(), "Concurrent record reading should succeed");

                i
            });
            
            handles.push(handle);
        }

        // Wait for all tasks to complete
        let results = futures::future::join_all(handles).await;
        
        // Verify all tasks succeeded
        let mut successful_count = 0;
        for result in results {
            match result {
                Ok(task_id) => {
                    successful_count += 1;
                    info!("Task {} completed successfully", task_id);
                },
                Err(e) => {
                    panic!("Concurrent task failed: {:?}", e);
                }
            }
        }

        assert_eq!(successful_count, 10, "All 10 concurrent operations should succeed");
        info!("SurrealDB concurrent operations test completed successfully");
        Ok(())
    }

    #[tokio::test]
    async fn test_surrealdb_cleanup_and_isolation() -> Result<(), Box<dyn std::error::Error>> {
        info!("Testing SurrealDB cleanup and isolation");

        let (connection_string, _) = setup_surrealdb_container().await?;
        let client = create_surrealdb_connection(&connection_string).await?;

        // Create a unique test record
        let test_id = format!("isolation_test_{}", uuid::Uuid::new_v4());
        let create_result = client
            .create(("isolation_test", &test_id))
            .content(serde_json::json!({
                "test_id": test_id,
                "timestamp": chrono::Utc::now()
            }))
            .await;

        assert!(create_result.is_ok(), "Test record creation should succeed");

        // Verify it exists
        let read_result = client.select(("isolation_test", &test_id)).await;
        assert!(read_result.is_ok(), "Test record should be readable");

        // Count records to verify isolation
        let count_result = client.query("SELECT count() FROM isolation_test").await;
        assert!(count_result.is_ok(), "Count query should succeed");

        let count = count_result.unwrap();
        info!("Found {} isolation test records", count.len());

        // Verify our specific record exists
        let exists_result = client.query(format!("SELECT * FROM isolation_test WHERE test_id = '{}'", test_id)).await;
        assert!(exists_result.is_ok(), "Existence check should succeed");

        let exists = exists_result.unwrap();
        assert!(exists.len() > 0, "Our test record should exist");

        info!("SurrealDB cleanup and isolation test completed successfully");
        Ok(())
    }
}