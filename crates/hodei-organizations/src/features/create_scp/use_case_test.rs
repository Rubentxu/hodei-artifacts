#[cfg(test)]
mod tests {
    use crate::features::create_scp::use_case::{CreateScpUseCase, DeleteScpUseCase, UpdateScpUseCase, GetScpUseCase, ListScpsUseCase};
    use crate::features::create_scp::dto::{CreateScpCommand, DeleteScpCommand, UpdateScpCommand, GetScpQuery, ListScpsQuery, ScpDto};
    use crate::features::create_scp::error::{CreateScpError, DeleteScpError, UpdateScpError, GetScpError, ListScpsError};
    use crate::features::create_scp::mocks::MockScpPersister;
    use crate::shared::domain::{Hrn, Policy};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_create_scp_success() {
        let persister = MockScpPersister::new();
        let use_case = CreateScpUseCase::new(persister);
        
        let command = CreateScpCommand {
            scp_id: "test-scp".to_string(),
            scp_content: "permit(principal, action, resource);".to_string(),
            description: Some("Test SCP".to_string()),
        };
        
        let result = use_case.execute(command).await;
        assert!(result.is_ok());
        
        let scp_dto = result.unwrap();
        assert_eq!(scp_dto.id.to_string(), "hrn:organizations:scp:test-scp");
        assert_eq!(scp_dto.content, "permit(principal, action, resource);");
        assert_eq!(scp_dto.description, Some("Test SCP".to_string()));
    }

    #[tokio::test]
    async fn test_create_scp_invalid_content() {
        let persister = MockScpPersister::new();
        let use_case = CreateScpUseCase::new(persister);
        
        let command = CreateScpCommand {
            scp_id: "test-scp".to_string(),
            scp_content: "invalid scp content".to_string(),
            description: Some("Test SCP".to_string()),
        };
        
        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), CreateScpError::InvalidScpContent);
    }

    #[tokio::test]
    async fn test_create_scp_already_exists() {
        let mut scps = HashMap::new();
        let existing_scp = Policy {
            id: Hrn::new("organizations", "scp", "existing-scp"),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Existing SCP".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        scps.insert("existing-scp".to_string(), existing_scp);
        
        let persister = MockScpPersister::with_scps(scps);
        let use_case = CreateScpUseCase::new(persister);
        
        let command = CreateScpCommand {
            scp_id: "existing-scp".to_string(),
            scp_content: "permit(principal, action, resource);".to_string(),
            description: Some("Test SCP".to_string()),
        };
        
        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), CreateScpError::ScpAlreadyExists);
    }

    #[tokio::test]
    async fn test_delete_scp_success() {
        let mut scps = HashMap::new();
        let existing_scp = Policy {
            id: Hrn::new("organizations", "scp", "existing-scp"),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Existing SCP".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        scps.insert("existing-scp".to_string(), existing_scp);
        
        let persister = MockScpPersister::with_scps(scps);
        let use_case = DeleteScpUseCase::new(persister);
        
        let command = DeleteScpCommand {
            scp_id: "existing-scp".to_string(),
        };
        
        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_delete_scp_not_found() {
        let persister = MockScpPersister::new();
        let use_case = DeleteScpUseCase::new(persister);
        
        let command = DeleteScpCommand {
            scp_id: "non-existent-scp".to_string(),
        };
        
        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), DeleteScpError::ScpNotFound);
    }

    #[tokio::test]
    async fn test_update_scp_success() {
        let mut scps = HashMap::new();
        let existing_scp = Policy {
            id: Hrn::new("organizations", "scp", "existing-scp"),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Existing SCP".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        scps.insert("existing-scp".to_string(), existing_scp);
        
        let persister = MockScpPersister::with_scps(scps);
        let use_case = UpdateScpUseCase::new(persister);
        
        let command = UpdateScpCommand {
            scp_id: "existing-scp".to_string(),
            scp_content: "forbid(principal, action, resource);".to_string(),
            description: Some("Updated SCP".to_string()),
        };
        
        let result = use_case.execute(command).await;
        assert!(result.is_ok());
        
        let scp_dto = result.unwrap();
        assert_eq!(scp_dto.id.to_string(), "hrn:organizations:scp:existing-scp");
        assert_eq!(scp_dto.content, "forbid(principal, action, resource);");
        assert_eq!(scp_dto.description, Some("Updated SCP".to_string()));
    }

    #[tokio::test]
    async fn test_update_scp_not_found() {
        let persister = MockScpPersister::new();
        let use_case = UpdateScpUseCase::new(persister);
        
        let command = UpdateScpCommand {
            scp_id: "non-existent-scp".to_string(),
            scp_content: "permit(principal, action, resource);".to_string(),
            description: Some("Test SCP".to_string()),
        };
        
        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), UpdateScpError::ScpNotFound);
    }

    #[tokio::test]
    async fn test_update_scp_invalid_content() {
        let mut scps = HashMap::new();
        let existing_scp = Policy {
            id: Hrn::new("organizations", "scp", "existing-scp"),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Existing SCP".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        scps.insert("existing-scp".to_string(), existing_scp);
        
        let persister = MockScpPersister::with_scps(scps);
        let use_case = UpdateScpUseCase::new(persister);
        
        let command = UpdateScpCommand {
            scp_id: "existing-scp".to_string(),
            scp_content: "invalid scp content".to_string(),
            description: Some("Test SCP".to_string()),
        };
        
        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), UpdateScpError::InvalidScpContent);
    }

    #[tokio::test]
    async fn test_get_scp_success() {
        let mut scps = HashMap::new();
        let existing_scp = Policy {
            id: Hrn::new("organizations", "scp", "existing-scp"),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Existing SCP".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        scps.insert("existing-scp".to_string(), existing_scp.clone());
        
        let persister = MockScpPersister::with_scps(scps);
        let use_case = GetScpUseCase::new(persister);
        
        let query = GetScpQuery {
            scp_id: "existing-scp".to_string(),
        };
        
        let result = use_case.execute(query).await;
        assert!(result.is_ok());
        
        let scp_dto = result.unwrap();
        assert_eq!(scp_dto.id, existing_scp.id);
        assert_eq!(scp_dto.content, existing_scp.content);
        assert_eq!(scp_dto.description, existing_scp.description);
    }

    #[tokio::test]
    async fn test_get_scp_not_found() {
        let persister = MockScpPersister::new();
        let use_case = GetScpUseCase::new(persister);
        
        let query = GetScpQuery {
            scp_id: "non-existent-scp".to_string(),
        };
        
        let result = use_case.execute(query).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), GetScpError::ScpNotFound);
    }

    #[tokio::test]
    async fn test_list_scps_success() {
        let mut scps = HashMap::new();
        let scp1 = Policy {
            id: Hrn::new("organizations", "scp", "scp-1"),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("SCP 1".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let scp2 = Policy {
            id: Hrn::new("organizations", "scp", "scp-2"),
            content: "forbid(principal, action, resource);".to_string(),
            description: Some("SCP 2".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        scps.insert("scp-1".to_string(), scp1.clone());
        scps.insert("scp-2".to_string(), scp2.clone());
        
        let persister = MockScpPersister::with_scps(scps);
        let use_case = ListScpsUseCase::new(persister);
        
        let query = ListScpsQuery {
            limit: None,
            offset: None,
        };
        
        let result = use_case.execute(query).await;
        assert!(result.is_ok());
        
        let scp_dtos = result.unwrap();
        assert_eq!(scp_dtos.len(), 2);
    }
}
