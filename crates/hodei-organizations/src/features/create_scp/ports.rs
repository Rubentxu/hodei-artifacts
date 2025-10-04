use crate::shared::domain::scp::ServiceControlPolicy;
use crate::features::create_scp::error::CreateScpError;
use async_trait::async_trait;

#[async_trait]
pub trait ScpPersister {
    async fn save(&self, scp: ServiceControlPolicy) -> Result<(), CreateScpError>;
}
