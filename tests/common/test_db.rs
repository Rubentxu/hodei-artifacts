//! Test Database Setup with Testcontainers
//!
//! This module provides utilities for setting up isolated SurrealDB instances
//! for integration testing using testcontainers.

use surrealdb::Surreal;
use surrealdb::engine::remote::ws::{Client, Ws};
use surrealdb::opt::auth::Root;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::surrealdb::SurrealDb;

/// Test database configuration
pub struct TestDb {
    pub client: Surreal<Client>,
    _container: ContainerAsync<SurrealDb>,
}

impl TestDb {
    /// Create a new test database instance
    ///
    /// This will:
    /// 1. Start a SurrealDB container
    /// 2. Connect to it
    /// 3. Authenticate as root
    /// 4. Set up namespace and database
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Start SurrealDB container
        let container = SurrealDb::default().with_tag("latest").start().await?;

        // Get connection details
        let host = container.get_host().await?;
        let port = container.get_host_port_ipv4(8000).await?;
        let endpoint = format!("{}:{}", host, port);

        // Connect to the database
        let client = Surreal::new::<Ws>(endpoint).await?;

        // Sign in as root
        client
            .signin(Root {
                username: "root",
                password: "root",
            })
            .await?;

        // Use namespace and database
        client.use_ns("test").use_db("test").await?;

        Ok(TestDb {
            client,
            _container: container,
        })
    }

    /// Create a test database with a custom namespace and database name
    pub async fn with_ns_db(
        namespace: &str,
        database: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let container = SurrealDb::default().with_tag("latest").start().await?;

        let host = container.get_host().await?;
        let port = container.get_host_port_ipv4(8000).await?;
        let endpoint = format!("{}:{}", host, port);

        let client = Surreal::new::<Ws>(endpoint).await?;

        client
            .signin(Root {
                username: "root",
                password: "root",
            })
            .await?;

        client.use_ns(namespace).use_db(database).await?;

        Ok(TestDb {
            client,
            _container: container,
        })
    }

    /// Get a reference to the database client
    pub fn client(&self) -> &Surreal<Client> {
        &self.client
    }

    /// Clean all data from the database
    pub async fn clean(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Delete all records from all tables
        let tables = vec!["policy", "user", "group"];
        for table in tables {
            let _: Vec<()> = self.client.delete(table).await?;
        }
        Ok(())
    }

    /// Seed the database with initial test data
    pub async fn seed_policies(&self, count: usize) -> Result<(), Box<dyn std::error::Error>> {
        use kernel::HodeiPolicy;

        for i in 0..count {
            let policy = HodeiPolicy::new(
                format!("test-policy-{}", i),
                format!("permit(principal, action, resource);"),
            );
            let _: Option<HodeiPolicy> = self
                .client
                .create(("policy", policy.id()))
                .content(policy)
                .await?;
        }
        Ok(())
    }
}

/// Helper to create a test database for a specific test
///
/// This ensures each test gets a fresh, isolated database instance
pub async fn setup_test_db() -> TestDb {
    TestDb::new().await.expect("Failed to create test database")
}

/// Helper to create a test database with custom configuration
pub async fn setup_test_db_with_config(namespace: &str, database: &str) -> TestDb {
    TestDb::with_ns_db(namespace, database)
        .await
        .expect("Failed to create test database with custom config")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_db_creation() {
        let db = TestDb::new().await;
        assert!(db.is_ok(), "Failed to create test database");
    }

    #[tokio::test]
    async fn test_db_with_custom_ns_db() {
        let db = TestDb::with_ns_db("test_ns", "test_db").await;
        assert!(
            db.is_ok(),
            "Failed to create test database with custom ns/db"
        );
    }

    #[tokio::test]
    async fn test_db_clean() {
        let db = TestDb::new().await.expect("Failed to create test database");
        let result = db.clean().await;
        assert!(result.is_ok(), "Failed to clean database");
    }

    #[tokio::test]
    async fn test_db_seed_policies() {
        let db = TestDb::new().await.expect("Failed to create test database");
        let result = db.seed_policies(5).await;
        assert!(result.is_ok(), "Failed to seed policies");
    }
}
