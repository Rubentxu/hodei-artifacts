use crate::features::create_scp::dto::{
    CreateScpCommand, DeleteScpCommand, GetScpQuery, ListScpsQuery, ScpDto, UpdateScpCommand,
};
use crate::features::create_scp::error::{
    CreateScpError, DeleteScpError, GetScpError, ListScpsError, UpdateScpError,
};
use crate::internal::domain::ServiceControlPolicy;
use async_trait::async_trait;
use kernel::Hrn;

/// Port for persisting Service Control Policies
///
/// This trait defines the interface for SCP persistence operations,
/// following the port-adapter pattern for clean architecture.
#[async_trait]
pub trait ScpPersister: Send + Sync {
    /// Create a new Service Control Policy
    async fn create_scp(&self, command: CreateScpCommand) -> Result<ScpDto, CreateScpError>;

    /// Delete an existing Service Control Policy
    async fn delete_scp(&self, command: DeleteScpCommand) -> Result<(), DeleteScpError>;

    /// Update an existing Service Control Policy
    async fn update_scp(&self, command: UpdateScpCommand) -> Result<ScpDto, UpdateScpError>;

    /// Get a specific Service Control Policy by HRN
    async fn get_scp(&self, query: GetScpQuery) -> Result<ScpDto, GetScpError>;

    /// List all Service Control Policies with optional pagination
    async fn list_scps(&self, query: ListScpsQuery) -> Result<Vec<ScpDto>, ListScpsError>;
}

/// Port for retrieving Service Control Policies for evaluation
///
/// This trait is used by the authorization engine to retrieve SCPs
/// for evaluation purposes.
#[async_trait]
pub trait ScpRepository: Send + Sync {
    /// Get a Service Control Policy by HRN
    async fn get_by_hrn(&self, hrn: &Hrn) -> Result<ServiceControlPolicy, GetScpError>;

    /// Get all Service Control Policies attached to an Account
    async fn get_scps_for_account(
        &self,
        account_hrn: &Hrn,
    ) -> Result<Vec<ServiceControlPolicy>, ListScpsError>;

    /// Get all Service Control Policies attached to an Organizational Unit
    async fn get_scps_for_ou(
        &self,
        ou_hrn: &Hrn,
    ) -> Result<Vec<ServiceControlPolicy>, ListScpsError>;

    /// Get all Service Control Policies in the hierarchy for an Account
    /// (includes SCPs from parent OUs up to the root)
    async fn get_effective_scps_for_account(
        &self,
        account_hrn: &Hrn,
    ) -> Result<Vec<ServiceControlPolicy>, ListScpsError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock implementation for testing
    struct MockScpPersister;

    #[async_trait]
    impl ScpPersister for MockScpPersister {
        async fn create_scp(&self, _command: CreateScpCommand) -> Result<ScpDto, CreateScpError> {
            unimplemented!("Mock implementation")
        }

        async fn delete_scp(&self, _command: DeleteScpCommand) -> Result<(), DeleteScpError> {
            unimplemented!("Mock implementation")
        }

        async fn update_scp(&self, _command: UpdateScpCommand) -> Result<ScpDto, UpdateScpError> {
            unimplemented!("Mock implementation")
        }

        async fn get_scp(&self, _query: GetScpQuery) -> Result<ScpDto, GetScpError> {
            unimplemented!("Mock implementation")
        }

        async fn list_scps(&self, _query: ListScpsQuery) -> Result<Vec<ScpDto>, ListScpsError> {
            unimplemented!("Mock implementation")
        }
    }

    struct MockScpRepository;

    #[async_trait]
    impl ScpRepository for MockScpRepository {
        async fn get_by_hrn(&self, _hrn: &Hrn) -> Result<ServiceControlPolicy, GetScpError> {
            unimplemented!("Mock implementation")
        }

        async fn get_scps_for_account(
            &self,
            _account_hrn: &Hrn,
        ) -> Result<Vec<ServiceControlPolicy>, ListScpsError> {
            unimplemented!("Mock implementation")
        }

        async fn get_scps_for_ou(
            &self,
            _ou_hrn: &Hrn,
        ) -> Result<Vec<ServiceControlPolicy>, ListScpsError> {
            unimplemented!("Mock implementation")
        }

        async fn get_effective_scps_for_account(
            &self,
            _account_hrn: &Hrn,
        ) -> Result<Vec<ServiceControlPolicy>, ListScpsError> {
            unimplemented!("Mock implementation")
        }
    }

    #[test]
    fn mock_persister_compiles() {
        let _persister = MockScpPersister;
        // If this compiles, the trait is properly defined
    }

    #[test]
    fn mock_repository_compiles() {
        let _repository = MockScpRepository;
        // If this compiles, the trait is properly defined
    }
}
