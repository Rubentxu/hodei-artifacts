#[cfg(test)]
mod tests {
    use super::super::dto::LoadSchemaCommand;
    use super::super::error::LoadSchemaError;
    use super::super::ports::SchemaStoragePort;
    use super::super::use_case::LoadSchemaUseCase;
    use crate::features::build_schema::error::BuildSchemaError;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    // Mock storage implementation for testing
    #[derive(Default)]
    #[allow(clippy::type_complexity)]
    struct MockSchemaStorage {
        schemas: Arc<Mutex<HashMap<String, (String, Option<String>)>>>,
        latest_id: Arc<Mutex<Option<String>>>,
    }

    impl MockSchemaStorage {
        fn new() -> Self {
            Self::default()
        }

        fn with_schemas(schemas: Vec<(String, String, Option<String>)>) -> Self {
            let mut map = HashMap::new();
            let mut latest = None;

            for (id, content, version) in schemas {
                map.insert(id.clone(), (content, version));
                latest = Some(id);
            }

            Self {
                schemas: Arc::new(Mutex::new(map)),
                latest_id: Arc::new(Mutex::new(latest)),
            }
        }

        fn add_schema(&self, id: String, content: String, version: Option<String>) {
            self.schemas
                .lock()
                .unwrap()
                .insert(id.clone(), (content, version));
            *self.latest_id.lock().unwrap() = Some(id);
        }
    }

    #[async_trait]
    impl SchemaStoragePort for MockSchemaStorage {
        async fn save_schema(
            &self,
            schema_json: String,
            version: Option<String>,
        ) -> Result<String, BuildSchemaError> {
            let schema_id = format!("schema_{}", self.schemas.lock().unwrap().len() + 1);
            self.add_schema(schema_id.clone(), schema_json, version);
            Ok(schema_id)
        }

        async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
            let latest_id = self.latest_id.lock().unwrap();
            if let Some(id) = latest_id.as_ref() {
                let schemas = self.schemas.lock().unwrap();
                Ok(schemas.get(id).map(|(content, _)| content.clone()))
            } else {
                Ok(None)
            }
        }

        async fn get_schema_by_version(
            &self,
            version: &str,
        ) -> Result<Option<String>, BuildSchemaError> {
            let schemas = self.schemas.lock().unwrap();
            Ok(schemas
                .values()
                .find(|(_, v)| v.as_deref() == Some(version))
                .map(|(content, _)| content.clone()))
        }

        async fn delete_schema(&self, schema_id: &str) -> Result<bool, BuildSchemaError> {
            let mut schemas = self.schemas.lock().unwrap();
            Ok(schemas.remove(schema_id).is_some())
        }

        async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
            let schemas = self.schemas.lock().unwrap();
            Ok(schemas.values().filter_map(|(_, v)| v.clone()).collect())
        }
    }

    fn create_use_case() -> LoadSchemaUseCase<MockSchemaStorage> {
        let storage = Arc::new(MockSchemaStorage::new());
        LoadSchemaUseCase::new(storage)
    }

    fn create_use_case_with_storage(
        storage: MockSchemaStorage,
    ) -> LoadSchemaUseCase<MockSchemaStorage> {
        LoadSchemaUseCase::new(Arc::new(storage))
    }

    #[tokio::test]
    async fn test_load_latest_schema() {
        let storage = MockSchemaStorage::with_schemas(vec![(
            "schema_1".to_string(),
            "Schema(...)".to_string(),
            Some("v1.0.0".to_string()),
        )]);
        let use_case = create_use_case_with_storage(storage);

        let command = LoadSchemaCommand::latest();
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.schema_id, "schema_latest");
        assert!(result.version.is_none());
    }

    #[tokio::test]
    async fn test_load_schema_by_version() {
        let storage = MockSchemaStorage::with_schemas(vec![
            (
                "schema_1".to_string(),
                "Schema(v1)".to_string(),
                Some("v1.0.0".to_string()),
            ),
            (
                "schema_2".to_string(),
                "Schema(v2)".to_string(),
                Some("v2.0.0".to_string()),
            ),
        ]);
        let use_case = create_use_case_with_storage(storage);

        let command = LoadSchemaCommand::with_version("v1.0.0");
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.version, Some("v1.0.0".to_string()));
    }

    #[tokio::test]
    async fn test_load_schema_not_found() {
        let use_case = create_use_case();

        let command = LoadSchemaCommand::latest();
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        match result {
            Err(LoadSchemaError::SchemaNotFound) => {}
            _ => panic!("Expected SchemaNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_load_invalid_version() {
        let storage = MockSchemaStorage::with_schemas(vec![(
            "schema_1".to_string(),
            "Schema(v1)".to_string(),
            Some("v1.0.0".to_string()),
        )]);
        let use_case = create_use_case_with_storage(storage);

        let command = LoadSchemaCommand::with_version("v99.0.0");
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        match result {
            Err(LoadSchemaError::InvalidSchemaVersion(_)) => {}
            _ => panic!("Expected InvalidSchemaVersion error"),
        }
    }

    #[tokio::test]
    async fn test_list_schema_versions() {
        let storage = MockSchemaStorage::with_schemas(vec![
            (
                "schema_1".to_string(),
                "Schema(v1)".to_string(),
                Some("v1.0.0".to_string()),
            ),
            (
                "schema_2".to_string(),
                "Schema(v2)".to_string(),
                Some("v2.0.0".to_string()),
            ),
            (
                "schema_3".to_string(),
                "Schema(v3)".to_string(),
                Some("v3.0.0".to_string()),
            ),
        ]);
        let use_case = create_use_case_with_storage(storage);

        let versions = use_case.list_versions().await;

        assert!(versions.is_ok());
        let versions = versions.unwrap();
        assert_eq!(versions.len(), 3);
        assert!(versions.contains(&"v1.0.0".to_string()));
        assert!(versions.contains(&"v2.0.0".to_string()));
        assert!(versions.contains(&"v3.0.0".to_string()));
    }

    #[tokio::test]
    async fn test_list_versions_empty() {
        let use_case = create_use_case();

        let versions = use_case.list_versions().await;

        assert!(versions.is_ok());
        assert_eq!(versions.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_load_command_latest() {
        let command = LoadSchemaCommand::latest();
        assert!(command.version.is_none());
    }

    #[tokio::test]
    async fn test_load_command_with_version() {
        let command = LoadSchemaCommand::with_version("v5.0.0");
        assert_eq!(command.version, Some("v5.0.0".to_string()));
    }

    #[tokio::test]
    async fn test_load_command_new_is_latest() {
        let command = LoadSchemaCommand::new();
        assert!(command.version.is_none());
    }

    #[tokio::test]
    async fn test_multiple_schema_versions() {
        let storage = MockSchemaStorage::with_schemas(vec![
            (
                "schema_1".to_string(),
                "Schema(v1)".to_string(),
                Some("v1.0.0".to_string()),
            ),
            (
                "schema_2".to_string(),
                "Schema(v2)".to_string(),
                Some("v2.0.0".to_string()),
            ),
        ]);
        let use_case = create_use_case_with_storage(storage);

        // Load v1
        let command = LoadSchemaCommand::with_version("v1.0.0");
        let result = use_case.execute(command).await;
        assert!(result.is_ok());

        // Load v2
        let command = LoadSchemaCommand::with_version("v2.0.0");
        let result = use_case.execute(command).await;
        assert!(result.is_ok());

        // Load latest (should be v2)
        let command = LoadSchemaCommand::latest();
        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_schema_without_version() {
        let storage = MockSchemaStorage::with_schemas(vec![(
            "schema_1".to_string(),
            "Schema(no version)".to_string(),
            None,
        )]);
        let use_case = create_use_case_with_storage(storage);

        let command = LoadSchemaCommand::latest();
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.version.is_none());
    }

    #[tokio::test]
    async fn test_load_schema_result_construction() {
        let storage = MockSchemaStorage::with_schemas(vec![(
            "schema_1".to_string(),
            "Schema(test)".to_string(),
            Some("test_version".to_string()),
        )]);
        let use_case = create_use_case_with_storage(storage);

        let command = LoadSchemaCommand::with_version("test_version");
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let result = result.unwrap();
        // Verify all fields are present
        assert_eq!(result.version, Some("test_version".to_string()));
        assert!(result.schema_id.starts_with("schema_"));
    }
}
