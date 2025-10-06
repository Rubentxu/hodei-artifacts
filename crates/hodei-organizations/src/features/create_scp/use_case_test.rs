#[cfg(test)]
mod tests {
    use crate::features::create_scp::dto::{
        CreateScpCommand, DeleteScpCommand, GetScpQuery, ListScpsQuery, ScpDto, UpdateScpCommand,
    };
    use crate::features::create_scp::error::{
        CreateScpError, DeleteScpError, GetScpError, ListScpsError, UpdateScpError,
    };
    use crate::features::create_scp::mocks::MockScpPersister;
    use crate::features::create_scp::use_case::{
        CreateScpUseCase, DeleteScpUseCase, GetScpUseCase, ListScpsUseCase, UpdateScpUseCase,
    };
    use crate::internal::domain::ServiceControlPolicy;
    use kernel::Hrn;
    use std::collections::HashMap;

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
    async fn test_create_scp_success() {
        let persister = MockScpPersister::new();
        let use_case = CreateScpUseCase::new(persister);

        let command = CreateScpCommand {
            hrn: create_test_hrn("test-scp"),
            name: "Test SCP".to_string(),
            document: "permit(principal, action, resource);".to_string(),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());

        let scp_dto = result.unwrap();
        assert!(scp_dto.hrn.to_string().contains("test-scp"));
        assert_eq!(scp_dto.name, "Test SCP");
        assert_eq!(scp_dto.document, "permit(principal, action, resource);");
    }

    #[tokio::test]
    async fn test_create_scp_invalid_content() {
        let persister = MockScpPersister::new();
        let use_case = CreateScpUseCase::new(persister);

        let command = CreateScpCommand {
            hrn: create_test_hrn("test-scp"),
            name: "Test SCP".to_string(),
            document: "invalid scp content".to_string(),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreateScpError::InvalidScpContent(_)
        ));
    }

    #[tokio::test]
    async fn test_create_scp_already_exists() {
        let mut scps = HashMap::new();
        let existing_scp = ServiceControlPolicy::new(
            create_test_hrn("existing-scp"),
            "Existing SCP".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        scps.insert(existing_scp.hrn.clone(), existing_scp);

        let persister = MockScpPersister::with_scps(scps);
        let use_case = CreateScpUseCase::new(persister);

        let command = CreateScpCommand {
            hrn: create_test_hrn("existing-scp"),
            name: "Test SCP".to_string(),
            document: "permit(principal, action, resource);".to_string(),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreateScpError::ScpAlreadyExists(_)
        ));
    }

    #[tokio::test]
    async fn test_delete_scp_success() {
        let mut scps = HashMap::new();
        let existing_scp = ServiceControlPolicy::new(
            create_test_hrn("existing-scp"),
            "Existing SCP".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let hrn_to_delete = existing_scp.hrn.clone();
        scps.insert(existing_scp.hrn.clone(), existing_scp);

        let persister = MockScpPersister::with_scps(scps);
        let use_case = DeleteScpUseCase::new(persister);

        let command = DeleteScpCommand { hrn: hrn_to_delete };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_delete_scp_not_found() {
        let persister = MockScpPersister::new();
        let use_case = DeleteScpUseCase::new(persister);

        let command = DeleteScpCommand {
            hrn: create_test_hrn("non-existent-scp"),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DeleteScpError::ScpNotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_update_scp_success() {
        let mut scps = HashMap::new();
        let existing_scp = ServiceControlPolicy::new(
            create_test_hrn("existing-scp"),
            "Existing SCP".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let hrn_to_update = existing_scp.hrn.clone();
        scps.insert(existing_scp.hrn.clone(), existing_scp);

        let persister = MockScpPersister::with_scps(scps);
        let use_case = UpdateScpUseCase::new(persister);

        let command = UpdateScpCommand {
            hrn: hrn_to_update.clone(),
            name: Some("Updated SCP".to_string()),
            document: Some("forbid(principal, action, resource);".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());

        let scp_dto = result.unwrap();
        assert_eq!(scp_dto.hrn, hrn_to_update);
        assert_eq!(scp_dto.name, "Updated SCP");
        assert_eq!(scp_dto.document, "forbid(principal, action, resource);");
    }

    #[tokio::test]
    async fn test_update_scp_not_found() {
        let persister = MockScpPersister::new();
        let use_case = UpdateScpUseCase::new(persister);

        let command = UpdateScpCommand {
            hrn: create_test_hrn("non-existent-scp"),
            name: Some("Test SCP".to_string()),
            document: Some("permit(principal, action, resource);".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UpdateScpError::ScpNotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_update_scp_invalid_content() {
        let mut scps = HashMap::new();
        let existing_scp = ServiceControlPolicy::new(
            create_test_hrn("existing-scp"),
            "Existing SCP".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let hrn_to_update = existing_scp.hrn.clone();
        scps.insert(existing_scp.hrn.clone(), existing_scp);

        let persister = MockScpPersister::with_scps(scps);
        let use_case = UpdateScpUseCase::new(persister);

        let command = UpdateScpCommand {
            hrn: hrn_to_update,
            name: Some("Test SCP".to_string()),
            document: Some("invalid scp content".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UpdateScpError::InvalidScpContent(_)
        ));
    }

    #[tokio::test]
    async fn test_get_scp_success() {
        let mut scps = HashMap::new();
        let existing_scp = ServiceControlPolicy::new(
            create_test_hrn("existing-scp"),
            "Existing SCP".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let hrn_to_get = existing_scp.hrn.clone();
        scps.insert(existing_scp.hrn.clone(), existing_scp.clone());

        let persister = MockScpPersister::with_scps(scps);
        let use_case = GetScpUseCase::new(persister);

        let query = GetScpQuery {
            hrn: hrn_to_get.clone(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());

        let scp_dto = result.unwrap();
        assert_eq!(scp_dto.hrn, hrn_to_get);
        assert_eq!(scp_dto.name, existing_scp.name);
        assert_eq!(scp_dto.document, existing_scp.document);
    }

    #[tokio::test]
    async fn test_get_scp_not_found() {
        let persister = MockScpPersister::new();
        let use_case = GetScpUseCase::new(persister);

        let query = GetScpQuery {
            hrn: create_test_hrn("non-existent-scp"),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), GetScpError::ScpNotFound(_)));
    }

    #[tokio::test]
    async fn test_list_scps_success() {
        let mut scps = HashMap::new();
        let scp1 = ServiceControlPolicy::new(
            create_test_hrn("scp-1"),
            "SCP 1".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let scp2 = ServiceControlPolicy::new(
            create_test_hrn("scp-2"),
            "SCP 2".to_string(),
            "forbid(principal, action, resource);".to_string(),
        );
        scps.insert(scp1.hrn.clone(), scp1.clone());
        scps.insert(scp2.hrn.clone(), scp2.clone());

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

    #[tokio::test]
    async fn test_list_scps_with_limit() {
        let mut scps = HashMap::new();
        for i in 1..=5 {
            let scp = ServiceControlPolicy::new(
                create_test_hrn(&format!("scp-{}", i)),
                format!("SCP {}", i),
                "permit(principal, action, resource);".to_string(),
            );
            scps.insert(scp.hrn.clone(), scp);
        }

        let persister = MockScpPersister::with_scps(scps);
        let use_case = ListScpsUseCase::new(persister);

        let query = ListScpsQuery {
            limit: Some(3),
            offset: None,
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());

        let scp_dtos = result.unwrap();
        assert_eq!(scp_dtos.len(), 3);
    }

    #[tokio::test]
    async fn test_list_scps_with_offset() {
        let mut scps = HashMap::new();
        for i in 1..=5 {
            let scp = ServiceControlPolicy::new(
                create_test_hrn(&format!("scp-{}", i)),
                format!("SCP {}", i),
                "permit(principal, action, resource);".to_string(),
            );
            scps.insert(scp.hrn.clone(), scp);
        }

        let persister = MockScpPersister::with_scps(scps);
        let use_case = ListScpsUseCase::new(persister);

        let query = ListScpsQuery {
            limit: None,
            offset: Some(2),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());

        let scp_dtos = result.unwrap();
        assert_eq!(scp_dtos.len(), 3);
    }
}
