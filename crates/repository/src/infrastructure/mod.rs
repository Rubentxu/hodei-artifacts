
// crates/repository/src/infrastructure/mod.rs

pub mod mongodb_adapter;

#[cfg(test)]
pub mod tests {
    use mongodb::{Client, Database};
    use std::sync::Once;

    static INIT: Once = Once::new();

    fn initialize() {
        INIT.call_once(|| {
            // Setup code here, like initializing logging
            let _ = tracing_subscriber::fmt::try_init();
        });
    }

    pub async fn setup_test_database() -> mongodb::error::Result<Database> {
        initialize();
        let client = Client::with_uri_str("mongodb://localhost:27017").await?;
        let db_name = format!("test_db_{}", uuid::Uuid::new_v4());
        Ok(client.database(&db_name))
    }
}
