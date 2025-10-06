use crate::features::create_scp::dto::{
    CreateScpCommand, DeleteScpCommand, GetScpQuery, ListScpsQuery, ScpDto, UpdateScpCommand,
};
use crate::features::create_scp::error::{
    CreateScpError, DeleteScpError, GetScpError, ListScpsError, UpdateScpError,
};
use crate::features::create_scp::ports::ScpPersister;
use async_trait::async_trait;
use tracing::instrument;

/// Use case for creating a new Service Control Policy
pub struct CreateScpUseCase<P: ScpPersister> {
    persister: P,
}

impl<P: ScpPersister> CreateScpUseCase<P> {
    pub fn new(persister: P) -> Self {
        Self { persister }
    }

    #[instrument(skip(self), fields(hrn = %command.hrn, name = %command.name))]
    pub async fn execute(&self, command: CreateScpCommand) -> Result<ScpDto, CreateScpError> {
        // Validate command
        if command.name.is_empty() {
            return Err(CreateScpError::ValidationError(
                "SCP name cannot be empty".to_string(),
            ));
        }

        if command.document.is_empty() {
            return Err(CreateScpError::ValidationError(
                "SCP document cannot be empty".to_string(),
            ));
        }

        // Basic Cedar policy validation - check for common patterns
        if !command.document.contains("permit") && !command.document.contains("forbid") {
            return Err(CreateScpError::InvalidScpContent(
                "Policy must contain at least one permit or forbid statement".to_string(),
            ));
        }

        // Delegate persistence to adapter
        self.persister.create_scp(command).await
    }
}

/// Use case for deleting an existing Service Control Policy
pub struct DeleteScpUseCase<P: ScpPersister> {
    persister: P,
}

impl<P: ScpPersister> DeleteScpUseCase<P> {
    pub fn new(persister: P) -> Self {
        Self { persister }
    }

    #[instrument(skip(self), fields(hrn = %command.hrn))]
    pub async fn execute(&self, command: DeleteScpCommand) -> Result<(), DeleteScpError> {
        self.persister.delete_scp(command).await
    }
}

/// Use case for updating an existing Service Control Policy
pub struct UpdateScpUseCase<P: ScpPersister> {
    persister: P,
}

impl<P: ScpPersister> UpdateScpUseCase<P> {
    pub fn new(persister: P) -> Self {
        Self { persister }
    }

    #[instrument(skip(self), fields(hrn = %command.hrn))]
    pub async fn execute(&self, command: UpdateScpCommand) -> Result<ScpDto, UpdateScpError> {
        // Validate that at least one field is being updated
        if command.name.is_none() && command.document.is_none() {
            return Err(UpdateScpError::NoUpdatesProvided);
        }

        // Validate document if provided
        if let Some(ref document) = command.document {
            if document.is_empty() {
                return Err(UpdateScpError::ValidationError(
                    "SCP document cannot be empty".to_string(),
                ));
            }

            if !document.contains("permit") && !document.contains("forbid") {
                return Err(UpdateScpError::InvalidScpContent(
                    "Policy must contain at least one permit or forbid statement".to_string(),
                ));
            }
        }

        // Validate name if provided
        if let Some(ref name) = command.name {
            if name.is_empty() {
                return Err(UpdateScpError::ValidationError(
                    "SCP name cannot be empty".to_string(),
                ));
            }
        }

        self.persister.update_scp(command).await
    }
}

/// Use case for retrieving a specific Service Control Policy
pub struct GetScpUseCase<P: ScpPersister> {
    persister: P,
}

impl<P: ScpPersister> GetScpUseCase<P> {
    pub fn new(persister: P) -> Self {
        Self { persister }
    }

    #[instrument(skip(self), fields(hrn = %query.hrn))]
    pub async fn execute(&self, query: GetScpQuery) -> Result<ScpDto, GetScpError> {
        self.persister.get_scp(query).await
    }
}

/// Use case for listing Service Control Policies
pub struct ListScpsUseCase<P: ScpPersister> {
    persister: P,
}

impl<P: ScpPersister> ListScpsUseCase<P> {
    pub fn new(persister: P) -> Self {
        Self { persister }
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, query: ListScpsQuery) -> Result<Vec<ScpDto>, ListScpsError> {
        // Validate pagination parameters
        if let Some(limit) = query.limit {
            if limit == 0 || limit > 1000 {
                return Err(ListScpsError::InvalidPagination(
                    "Limit must be between 1 and 1000".to_string(),
                ));
            }
        }

        self.persister.list_scps(query).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::Hrn;

    // Mock persister for testing
    struct MockScpPersister {
        should_fail: bool,
        existing_hrns: Vec<Hrn>,
    }

    impl MockScpPersister {
        fn new() -> Self {
            Self {
                should_fail: false,
                existing_hrns: Vec::new(),
            }
        }

        fn with_failure() -> Self {
            Self {
                should_fail: true,
                existing_hrns: Vec::new(),
            }
        }

        fn with_existing_hrn(hrn: Hrn) -> Self {
            Self {
                should_fail: false,
                existing_hrns: vec![hrn],
            }
        }
    }

    #[async_trait]
    impl ScpPersister for MockScpPersister {
        async fn create_scp(&self, command: CreateScpCommand) -> Result<ScpDto, CreateScpError> {
            if self.should_fail {
                return Err(CreateScpError::StorageError("Mock failure".to_string()));
            }

            if self.existing_hrns.contains(&command.hrn) {
                return Err(CreateScpError::ScpAlreadyExists(command.hrn.to_string()));
            }

            Ok(ScpDto {
                hrn: command.hrn,
                name: command.name,
                document: command.document,
            })
        }

        async fn delete_scp(&self, command: DeleteScpCommand) -> Result<(), DeleteScpError> {
            if self.should_fail {
                return Err(DeleteScpError::StorageError("Mock failure".to_string()));
            }

            if !self.existing_hrns.contains(&command.hrn) {
                return Err(DeleteScpError::ScpNotFound(command.hrn.to_string()));
            }

            Ok(())
        }

        async fn update_scp(&self, command: UpdateScpCommand) -> Result<ScpDto, UpdateScpError> {
            if self.should_fail {
                return Err(UpdateScpError::StorageError("Mock failure".to_string()));
            }

            if !self.existing_hrns.contains(&command.hrn) {
                return Err(UpdateScpError::ScpNotFound(command.hrn.to_string()));
            }

            Ok(ScpDto {
                hrn: command.hrn.clone(),
                name: command.name.unwrap_or_else(|| "Updated".to_string()),
                document: command
                    .document
                    .unwrap_or_else(|| "permit(principal, action, resource);".to_string()),
            })
        }

        async fn get_scp(&self, query: GetScpQuery) -> Result<ScpDto, GetScpError> {
            if self.should_fail {
                return Err(GetScpError::StorageError("Mock failure".to_string()));
            }

            if !self.existing_hrns.contains(&query.hrn) {
                return Err(GetScpError::ScpNotFound(query.hrn.to_string()));
            }

            Ok(ScpDto {
                hrn: query.hrn,
                name: "TestPolicy".to_string(),
                document: "permit(principal, action, resource);".to_string(),
            })
        }

        async fn list_scps(&self, _query: ListScpsQuery) -> Result<Vec<ScpDto>, ListScpsError> {
            if self.should_fail {
                return Err(ListScpsError::StorageError("Mock failure".to_string()));
            }

            Ok(self
                .existing_hrns
                .iter()
                .map(|hrn| ScpDto {
                    hrn: hrn.clone(),
                    name: "TestPolicy".to_string(),
                    document: "permit(principal, action, resource);".to_string(),
                })
                .collect())
        }
    }

    fn sample_hrn() -> Hrn {
        Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "ServiceControlPolicy".to_string(),
            "scp-123".to_string(),
        )
    }

    fn sample_create_command() -> CreateScpCommand {
        CreateScpCommand {
            hrn: sample_hrn(),
            name: "TestPolicy".to_string(),
            document: "permit(principal, action, resource);".to_string(),
        }
    }

    // CreateScpUseCase Tests
    #[tokio::test]
    async fn create_scp_success() {
        let persister = MockScpPersister::new();
        let use_case = CreateScpUseCase::new(persister);
        let command = sample_create_command();

        let result = use_case.execute(command).await;
        assert!(result.is_ok());

        let dto = result.unwrap();
        assert_eq!(dto.name, "TestPolicy");
    }

    #[tokio::test]
    async fn create_scp_validates_empty_name() {
        let persister = MockScpPersister::new();
        let use_case = CreateScpUseCase::new(persister);
        let mut command = sample_create_command();
        command.name = String::new();

        let result = use_case.execute(command).await;
        assert!(matches!(result, Err(CreateScpError::ValidationError(_))));
    }

    #[tokio::test]
    async fn create_scp_validates_empty_document() {
        let persister = MockScpPersister::new();
        let use_case = CreateScpUseCase::new(persister);
        let mut command = sample_create_command();
        command.document = String::new();

        let result = use_case.execute(command).await;
        assert!(matches!(result, Err(CreateScpError::ValidationError(_))));
    }

    #[tokio::test]
    async fn create_scp_validates_document_content() {
        let persister = MockScpPersister::new();
        let use_case = CreateScpUseCase::new(persister);
        let mut command = sample_create_command();
        command.document = "invalid policy content".to_string();

        let result = use_case.execute(command).await;
        assert!(matches!(result, Err(CreateScpError::InvalidScpContent(_))));
    }

    #[tokio::test]
    async fn create_scp_handles_already_exists_error() {
        let hrn = sample_hrn();
        let persister = MockScpPersister::with_existing_hrn(hrn.clone());
        let use_case = CreateScpUseCase::new(persister);
        let command = sample_create_command();

        let result = use_case.execute(command).await;
        assert!(matches!(result, Err(CreateScpError::ScpAlreadyExists(_))));
    }

    // DeleteScpUseCase Tests
    #[tokio::test]
    async fn delete_scp_success() {
        let hrn = sample_hrn();
        let persister = MockScpPersister::with_existing_hrn(hrn.clone());
        let use_case = DeleteScpUseCase::new(persister);
        let command = DeleteScpCommand { hrn };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn delete_scp_not_found() {
        let persister = MockScpPersister::new();
        let use_case = DeleteScpUseCase::new(persister);
        let command = DeleteScpCommand { hrn: sample_hrn() };

        let result = use_case.execute(command).await;
        assert!(matches!(result, Err(DeleteScpError::ScpNotFound(_))));
    }

    // UpdateScpUseCase Tests
    #[tokio::test]
    async fn update_scp_success() {
        let hrn = sample_hrn();
        let persister = MockScpPersister::with_existing_hrn(hrn.clone());
        let use_case = UpdateScpUseCase::new(persister);
        let command = UpdateScpCommand {
            hrn,
            name: Some("UpdatedName".to_string()),
            document: None,
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn update_scp_validates_no_updates() {
        let persister = MockScpPersister::new();
        let use_case = UpdateScpUseCase::new(persister);
        let command = UpdateScpCommand {
            hrn: sample_hrn(),
            name: None,
            document: None,
        };

        let result = use_case.execute(command).await;
        assert!(matches!(result, Err(UpdateScpError::NoUpdatesProvided)));
    }

    #[tokio::test]
    async fn update_scp_validates_empty_document() {
        let persister = MockScpPersister::new();
        let use_case = UpdateScpUseCase::new(persister);
        let command = UpdateScpCommand {
            hrn: sample_hrn(),
            name: None,
            document: Some(String::new()),
        };

        let result = use_case.execute(command).await;
        assert!(matches!(result, Err(UpdateScpError::ValidationError(_))));
    }

    #[tokio::test]
    async fn update_scp_validates_document_content() {
        let persister = MockScpPersister::new();
        let use_case = UpdateScpUseCase::new(persister);
        let command = UpdateScpCommand {
            hrn: sample_hrn(),
            name: None,
            document: Some("invalid content".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(matches!(result, Err(UpdateScpError::InvalidScpContent(_))));
    }

    // GetScpUseCase Tests
    #[tokio::test]
    async fn get_scp_success() {
        let hrn = sample_hrn();
        let persister = MockScpPersister::with_existing_hrn(hrn.clone());
        let use_case = GetScpUseCase::new(persister);
        let query = GetScpQuery { hrn };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn get_scp_not_found() {
        let persister = MockScpPersister::new();
        let use_case = GetScpUseCase::new(persister);
        let query = GetScpQuery { hrn: sample_hrn() };

        let result = use_case.execute(query).await;
        assert!(matches!(result, Err(GetScpError::ScpNotFound(_))));
    }

    // ListScpsUseCase Tests
    #[tokio::test]
    async fn list_scps_success() {
        let persister = MockScpPersister::new();
        let use_case = ListScpsUseCase::new(persister);
        let query = ListScpsQuery {
            limit: Some(10),
            offset: Some(0),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn list_scps_validates_limit_zero() {
        let persister = MockScpPersister::new();
        let use_case = ListScpsUseCase::new(persister);
        let query = ListScpsQuery {
            limit: Some(0),
            offset: None,
        };

        let result = use_case.execute(query).await;
        assert!(matches!(result, Err(ListScpsError::InvalidPagination(_))));
    }

    #[tokio::test]
    async fn list_scps_validates_limit_too_large() {
        let persister = MockScpPersister::new();
        let use_case = ListScpsUseCase::new(persister);
        let query = ListScpsQuery {
            limit: Some(1001),
            offset: None,
        };

        let result = use_case.execute(query).await;
        assert!(matches!(result, Err(ListScpsError::InvalidPagination(_))));
    }
}
