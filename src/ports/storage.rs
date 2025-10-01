
use crate::error::Result;
use std::fmt::Debug;

// Trait para abstraer el adaptador de almacenamiento
#[async_trait::async_trait]
pub trait StorageAdapterPort: Debug + Send + Sync {
    async fn connect(url: &str) -> Result<Self>
    where
        Self: Sized;
    async fn health_check(&self) -> Result<bool>;
}
