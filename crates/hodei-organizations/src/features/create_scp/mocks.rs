use crate::features::create_scp::dto::{
    CreateScpCommand, DeleteScpCommand, GetScpQuery, ListScpsQuery, ScpDto, UpdateScpCommand,
};
use crate::features::create_scp::error::{
    CreateScpError, DeleteScpError, GetScpError, ListScpsError, UpdateScpError,
};
use crate::features::create_scp::ports::ScpPersister;
use crate::internal::domain::scp::ServiceControlPolicy;
use async_trait::async_trait;
use kernel::Hrn;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct MockScpPersister {
    scps: Arc<Mutex<HashMap<Hrn, ServiceControlPolicy>>>,
}

impl MockScpPersister {
    pub fn new() -> Self {
        Self {
            scps: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_scps(scps: HashMap<Hrn, ServiceControlPolicy>) -> Self {
        Self {
            scps: Arc::new(Mutex::new(scps)),
        }
    }
}

#[async_trait]
impl ScpPersister for MockScpPersister {
    async fn create_scp(&self, command: CreateScpCommand) -> Result<ScpDto, CreateScpError> {
        let mut scps = self.scps.lock().await;

        // Check if SCP already exists
        if scps.contains_key(&command.hrn) {
            return Err(CreateScpError::ScpAlreadyExists(command.hrn.to_string()));
        }

        // Basic validation: check if document is not empty and contains basic Cedar syntax
        if command.document.trim().is_empty()
            || (!command.document.contains("permit") && !command.document.contains("forbid"))
        {
            return Err(CreateScpError::InvalidScpContent(
                "Document must contain 'permit' or 'forbid' statements".to_string(),
            ));
        }

        let scp = ServiceControlPolicy::new(command.hrn.clone(), command.name, command.document);

        scps.insert(command.hrn.clone(), scp.clone());
        Ok(ScpDto::from(scp))
    }

    async fn delete_scp(&self, command: DeleteScpCommand) -> Result<(), DeleteScpError> {
        let mut scps = self.scps.lock().await;

        if !scps.contains_key(&command.hrn) {
            return Err(DeleteScpError::ScpNotFound(command.hrn.to_string()));
        }

        scps.remove(&command.hrn);
        Ok(())
    }

    async fn update_scp(&self, command: UpdateScpCommand) -> Result<ScpDto, UpdateScpError> {
        let mut scps = self.scps.lock().await;

        let scp = scps
            .get(&command.hrn)
            .ok_or_else(|| UpdateScpError::ScpNotFound(command.hrn.to_string()))?;

        // Get new values or keep existing ones
        let new_name = command.name.unwrap_or_else(|| scp.name.clone());
        let new_document = command.document.unwrap_or_else(|| scp.document.clone());

        // Basic validation: check if document is not empty and contains basic Cedar syntax
        if new_document.trim().is_empty()
            || (!new_document.contains("permit") && !new_document.contains("forbid"))
        {
            return Err(UpdateScpError::InvalidScpContent(
                "Document must contain 'permit' or 'forbid' statements".to_string(),
            ));
        }

        // Update the SCP
        let updated_scp = ServiceControlPolicy::new(command.hrn.clone(), new_name, new_document);

        scps.insert(command.hrn.clone(), updated_scp.clone());
        Ok(ScpDto::from(updated_scp))
    }

    async fn get_scp(&self, query: GetScpQuery) -> Result<ScpDto, GetScpError> {
        let scps = self.scps.lock().await;
        scps.get(&query.hrn)
            .cloned()
            .map(ScpDto::from)
            .ok_or_else(|| GetScpError::ScpNotFound(query.hrn.to_string()))
    }

    async fn list_scps(&self, query: ListScpsQuery) -> Result<Vec<ScpDto>, ListScpsError> {
        let scps = self.scps.lock().await;
        let mut result: Vec<ServiceControlPolicy> = scps.values().cloned().collect();

        // Apply offset if specified
        if let Some(offset) = query.offset {
            result = result.into_iter().skip(offset as usize).collect();
        }

        // Apply limit if specified
        if let Some(limit) = query.limit {
            result = result.into_iter().take(limit as usize).collect();
        }

        Ok(result.into_iter().map(ScpDto::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_hrn(resource_id: &str) -> Hrn {
        Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "ServiceControlPolicy".to_string(),
            resource_id.to_string(),
        )
    }

    #[tokio::test]
    async fn test_mock_persister_create_scp() {
        let persister = MockScpPersister::new();
        let command = CreateScpCommand {
            hrn: create_test_hrn("test-scp"),
            name: "Test SCP".to_string(),
            document: "permit(principal, action, resource);".to_string(),
        };

        let result = persister.create_scp(command).await;
        assert!(result.is_ok());

        let scp = result.unwrap();
        assert_eq!(scp.name, "Test SCP");
        assert_eq!(scp.document, "permit(principal, action, resource);");
    }

    #[tokio::test]
    async fn test_mock_persister_duplicate_scp() {
        let mut scps = HashMap::new();
        let hrn = create_test_hrn("existing-scp");
        let existing_scp = ServiceControlPolicy::new(
            hrn.clone(),
            "Existing".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        scps.insert(hrn.clone(), existing_scp);

        let persister = MockScpPersister::with_scps(scps);
        let command = CreateScpCommand {
            hrn,
            name: "Another SCP".to_string(),
            document: "permit(principal, action, resource);".to_string(),
        };

        let result = persister.create_scp(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreateScpError::ScpAlreadyExists(_)
        ));
    }

    #[tokio::test]
    async fn test_mock_persister_invalid_content() {
        let persister = MockScpPersister::new();
        let command = CreateScpCommand {
            hrn: create_test_hrn("test-scp"),
            name: "Test SCP".to_string(),
            document: "invalid content".to_string(),
        };

        let result = persister.create_scp(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreateScpError::InvalidScpContent(_)
        ));
    }

    #[tokio::test]
    async fn test_mock_persister_delete_scp() {
        let mut scps = HashMap::new();
        let hrn = create_test_hrn("existing-scp");
        let existing_scp = ServiceControlPolicy::new(
            hrn.clone(),
            "Existing".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        scps.insert(hrn.clone(), existing_scp);

        let persister = MockScpPersister::with_scps(scps);
        let command = DeleteScpCommand { hrn };

        let result = persister.delete_scp(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_persister_delete_nonexistent_scp() {
        let persister = MockScpPersister::new();
        let command = DeleteScpCommand {
            hrn: create_test_hrn("nonexistent"),
        };

        let result = persister.delete_scp(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DeleteScpError::ScpNotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_mock_persister_update_scp() {
        let mut scps = HashMap::new();
        let hrn = create_test_hrn("existing-scp");
        let existing_scp = ServiceControlPolicy::new(
            hrn.clone(),
            "Original Name".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        scps.insert(hrn.clone(), existing_scp);

        let persister = MockScpPersister::with_scps(scps);
        let command = UpdateScpCommand {
            hrn: hrn.clone(),
            name: Some("Updated Name".to_string()),
            document: Some("forbid(principal, action, resource);".to_string()),
        };

        let result = persister.update_scp(command).await;
        assert!(result.is_ok());

        let updated = result.unwrap();
        assert_eq!(updated.name, "Updated Name");
        assert_eq!(updated.document, "forbid(principal, action, resource);");
    }

    #[tokio::test]
    async fn test_mock_persister_get_scp() {
        let mut scps = HashMap::new();
        let hrn = create_test_hrn("existing-scp");
        let existing_scp = ServiceControlPolicy::new(
            hrn.clone(),
            "Test SCP".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        scps.insert(hrn.clone(), existing_scp.clone());

        let persister = MockScpPersister::with_scps(scps);
        let query = GetScpQuery { hrn: hrn.clone() };

        let result = persister.get_scp(query).await;
        assert!(result.is_ok());

        let retrieved = result.unwrap();
        assert_eq!(retrieved.hrn, hrn);
        assert_eq!(retrieved.name, "Test SCP");
    }

    #[tokio::test]
    async fn test_mock_persister_list_scps() {
        let mut scps = HashMap::new();
        for i in 1..=5 {
            let hrn = create_test_hrn(&format!("scp-{}", i));
            let scp = ServiceControlPolicy::new(
                hrn.clone(),
                format!("SCP {}", i),
                "permit(principal, action, resource);".to_string(),
            );
            scps.insert(hrn, scp);
        }

        let persister = MockScpPersister::with_scps(scps);
        let query = ListScpsQuery {
            limit: None,
            offset: None,
        };

        let result = persister.list_scps(query).await;
        assert!(result.is_ok());

        let list = result.unwrap();
        assert_eq!(list.len(), 5);
    }

    #[tokio::test]
    async fn test_mock_persister_list_scps_with_limit() {
        let mut scps = HashMap::new();
        for i in 1..=5 {
            let hrn = create_test_hrn(&format!("scp-{}", i));
            let scp = ServiceControlPolicy::new(
                hrn.clone(),
                format!("SCP {}", i),
                "permit(principal, action, resource);".to_string(),
            );
            scps.insert(hrn, scp);
        }

        let persister = MockScpPersister::with_scps(scps);
        let query = ListScpsQuery {
            limit: Some(3),
            offset: None,
        };

        let result = persister.list_scps(query).await;
        assert!(result.is_ok());

        let list = result.unwrap();
        assert_eq!(list.len(), 3);
    }
}
