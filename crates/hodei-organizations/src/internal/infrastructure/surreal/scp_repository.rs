use crate::internal::application::ports::scp_repository::{ScpRepository, ScpRepositoryError};
use crate::internal::domain::scp::ServiceControlPolicy;
use kernel::Hrn;
use surrealdb::RecordId;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;

/// SurrealDB implementation of ScpRepository
pub struct SurrealScpRepository {
    db: Surreal<Any>,
}

impl SurrealScpRepository {
    /// Create a new SurrealScpRepository instance
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl ScpRepository for SurrealScpRepository {
    /// Save a service control policy
    async fn save(&self, scp: &ServiceControlPolicy) -> Result<(), ScpRepositoryError> {
        let hrn_string = scp.hrn.to_string();
        let record_id = RecordId::from(("scp", hrn_string.as_str()));

        self.db
            .update::<Option<ServiceControlPolicy>>(record_id)
            .content(scp.clone())
            .await
            .map_err(|e| ScpRepositoryError::Storage(e.to_string()))?;

        Ok(())
    }

    /// Find a service control policy by HRN
    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<ServiceControlPolicy>, ScpRepositoryError> {
        let hrn_string = hrn.to_string();
        let record_id = RecordId::from(("scp", hrn_string.as_str()));

        let result = self
            .db
            .select::<Option<ServiceControlPolicy>>(record_id)
            .await
            .map_err(|e| ScpRepositoryError::Storage(e.to_string()))?;

        Ok(result)
    }
}
