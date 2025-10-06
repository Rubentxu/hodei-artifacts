//! Integration tests for SurrealDB adapter
//!
//! These tests verify that the kernel can create SurrealDB connections
//! and perform basic operations with the placeholder implementation.

use kernel::infrastructure::surrealdb_adapter::{SurrealDbConfig, SurrealDbConnection};

#[tokio::test]
async fn test_surrealdb_connection_creation() {
    // Test creating a SurrealDB connection with in-memory config
    let config = SurrealDbConfig {
        url: "memory".to_string(),
        namespace: "test".to_string(),
        database: "test".to_string(),
        username: None,
        password: None,
    };

    let result = SurrealDbConnection::new(config);

    assert!(
        result.is_ok(),
        "Should create SurrealDB connection: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_surrealdb_connect() {
    // Test connecting to SurrealDB (placeholder implementation)
    let config = SurrealDbConfig {
        url: "memory".to_string(),
        namespace: "test".to_string(),
        database: "test".to_string(),
        username: None,
        password: None,
    };

    let conn = SurrealDbConnection::new(config).expect("Failed to create connection");

    let result = conn.connect().await;

    assert!(
        result.is_ok(),
        "Should connect successfully: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_surrealdb_disconnect() {
    // Test disconnecting from SurrealDB
    let config = SurrealDbConfig {
        url: "memory".to_string(),
        namespace: "test".to_string(),
        database: "test".to_string(),
        username: None,
        password: None,
    };

    let conn = SurrealDbConnection::new(config).expect("Failed to create connection");
    conn.connect().await.expect("Failed to connect");

    let result = conn.disconnect().await;

    assert!(
        result.is_ok(),
        "Should disconnect successfully: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_surrealdb_config_default() {
    // Test default configuration
    let config = SurrealDbConfig::default();

    assert_eq!(config.url, "ws://localhost:8000");
    assert_eq!(config.namespace, "hodei");
    assert_eq!(config.database, "hodei");
    assert!(config.username.is_none());
    assert!(config.password.is_none());
}

#[tokio::test]
async fn test_surrealdb_custom_config() {
    // Test custom configuration
    let config = SurrealDbConfig {
        url: "ws://custom-host:9000".to_string(),
        namespace: "custom_ns".to_string(),
        database: "custom_db".to_string(),
        username: Some("admin".to_string()),
        password: Some("secret".to_string()),
    };

    let conn = SurrealDbConnection::new(config.clone());

    assert!(conn.is_ok(), "Should create connection with custom config");
}

#[tokio::test]
async fn test_surrealdb_multiple_connections() {
    // Test creating multiple connections
    let config1 = SurrealDbConfig {
        url: "memory".to_string(),
        namespace: "ns1".to_string(),
        database: "db1".to_string(),
        username: None,
        password: None,
    };

    let config2 = SurrealDbConfig {
        url: "memory".to_string(),
        namespace: "ns2".to_string(),
        database: "db2".to_string(),
        username: None,
        password: None,
    };

    let conn1 = SurrealDbConnection::new(config1).expect("Failed to create connection 1");
    let conn2 = SurrealDbConnection::new(config2).expect("Failed to create connection 2");

    let result1 = conn1.connect().await;
    let result2 = conn2.connect().await;

    assert!(result1.is_ok(), "Connection 1 should succeed");
    assert!(result2.is_ok(), "Connection 2 should succeed");
}
