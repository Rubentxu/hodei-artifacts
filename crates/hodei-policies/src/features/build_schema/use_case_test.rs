#[cfg(test)]
mod tests {
    use super::super::dto::BuildSchemaCommand;
    use super::super::error::BuildSchemaError;
    use super::super::ports::SchemaStoragePort;
    use super::super::use_case::BuildSchemaUseCase;
    use crate::internal::engine::builder::EngineBuilder;
    use async_trait::async_trait;
    use kernel::{
        ActionTrait, AttributeName, AttributeType, HodeiEntityType, ResourceTypeName, ServiceName,
    };
    use std::sync::{Arc, Mutex};

    // Mock storage implementation for testing
    #[derive(Default)]
    struct MockSchemaStorage {
        saved_schemas: Arc<Mutex<Vec<(String, Option<String>)>>>,
        should_fail: Arc<Mutex<bool>>,
    }

    impl MockSchemaStorage {
        fn new() -> Self {
            Self::default()
        }

        fn with_failure() -> Self {
            Self {
                saved_schemas: Arc::new(Mutex::new(Vec::new())),
                should_fail: Arc::new(Mutex::new(true)),
            }
        }

        fn get_saved_count(&self) -> usize {
            self.saved_schemas.lock().unwrap().len()
        }

        fn get_last_saved(&self) -> Option<(String, Option<String>)> {
            self.saved_schemas.lock().unwrap().last().cloned()
        }
    }

    #[async_trait]
    impl SchemaStoragePort for MockSchemaStorage {
        async fn save_schema(
            &self,
            schema_json: String,
            version: Option<String>,
        ) -> Result<String, BuildSchemaError> {
            if *self.should_fail.lock().unwrap() {
                return Err(BuildSchemaError::SchemaStorageError(
                    "Mock storage failure".to_string(),
                ));
            }

            self.saved_schemas
                .lock()
                .unwrap()
                .push((schema_json.clone(), version.clone()));

            let schema_id = format!("schema_{}", self.saved_schemas.lock().unwrap().len());
            Ok(schema_id)
        }

        async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
            Ok(self
                .saved_schemas
                .lock()
                .unwrap()
                .last()
                .map(|(json, _)| json.clone()))
        }

        async fn get_schema_by_version(
            &self,
            version: &str,
        ) -> Result<Option<String>, BuildSchemaError> {
            Ok(self
                .saved_schemas
                .lock()
                .unwrap()
                .iter()
                .find(|(_, v)| v.as_deref() == Some(version))
                .map(|(json, _)| json.clone()))
        }

        async fn delete_schema(&self, _schema_id: &str) -> Result<bool, BuildSchemaError> {
            Ok(true)
        }

        async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
            Ok(self
                .saved_schemas
                .lock()
                .unwrap()
                .iter()
                .filter_map(|(_, v)| v.clone())
                .collect())
        }
    }

    // Mock entity types
    struct MockUser;

    impl HodeiEntityType for MockUser {
        fn service_name() -> ServiceName {
            ServiceName::new("iam").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("User").unwrap()
        }

        fn is_principal_type() -> bool {
            true
        }

        fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
            vec![
                (AttributeName::new("name").unwrap(), AttributeType::String),
                (AttributeName::new("active").unwrap(), AttributeType::Bool),
            ]
        }
    }

    struct MockDocument;

    impl HodeiEntityType for MockDocument {
        fn service_name() -> ServiceName {
            ServiceName::new("storage").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("Document").unwrap()
        }

        fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
            vec![(AttributeName::new("title").unwrap(), AttributeType::String)]
        }
    }

    // Mock action types
    struct ReadAction;

    impl ActionTrait for ReadAction {
        fn name() -> &'static str {
            "Read"
        }

        fn service_name() -> ServiceName {
            ServiceName::new("storage").unwrap()
        }

        fn applies_to_principal() -> String {
            "Iam::User".to_string()
        }

        fn applies_to_resource() -> String {
            "Storage::Document".to_string()
        }
    }

    struct WriteAction;

    impl ActionTrait for WriteAction {
        fn name() -> &'static str {
            "Write"
        }

        fn service_name() -> ServiceName {
            ServiceName::new("storage").unwrap()
        }

        fn applies_to_principal() -> String {
            "Iam::User".to_string()
        }

        fn applies_to_resource() -> String {
            "Storage::Document".to_string()
        }
    }

    fn create_use_case() -> BuildSchemaUseCase<MockSchemaStorage> {
        let builder = Arc::new(Mutex::new(EngineBuilder::new()));
        let storage = Arc::new(MockSchemaStorage::new());
        BuildSchemaUseCase::new(builder, storage)
    }

    fn create_use_case_with_storage(
        storage: MockSchemaStorage,
    ) -> BuildSchemaUseCase<MockSchemaStorage> {
        let builder = Arc::new(Mutex::new(EngineBuilder::new()));
        BuildSchemaUseCase::new(builder, Arc::new(storage))
    }

    #[tokio::test]
    async fn test_build_schema_with_entities_and_actions() {
        let use_case = create_use_case();

        // Register some types
        {
            let mut builder = use_case.builder().lock().unwrap();
            builder.register_entity::<MockUser>().unwrap();
            builder.register_entity::<MockDocument>().unwrap();
            builder.register_action_type::<ReadAction>().unwrap();
            builder.register_action_type::<WriteAction>().unwrap();
        }

        let command = BuildSchemaCommand::new()
            .with_version("v1.0.0")
            .with_validation(true);

        let result = use_case.execute(command).await;

        assert!(result.is_ok(), "Failed to build schema: {:?}", result);
        let result = result.unwrap();
        assert_eq!(result.entity_count, 2);
        assert_eq!(result.action_count, 2);
        assert_eq!(result.version, Some("v1.0.0".to_string()));
        assert!(result.validated);
        assert_eq!(result.schema_id, "schema_1");
    }

    #[tokio::test]
    async fn test_build_schema_only_entities() {
        let use_case = create_use_case();

        {
            let mut builder = use_case.builder().lock().unwrap();
            builder.register_entity::<MockUser>().unwrap();
        }

        let command = BuildSchemaCommand::new();
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.entity_count, 1);
        assert_eq!(result.action_count, 0);
    }

    #[tokio::test]
    async fn test_build_schema_with_actions_and_referenced_entities() {
        let use_case = create_use_case();

        {
            let mut builder = use_case.builder().lock().unwrap();
            // Actions reference entity types, so we need to register those too
            builder.register_entity::<MockUser>().unwrap();
            builder.register_entity::<MockDocument>().unwrap();
            builder.register_action_type::<ReadAction>().unwrap();
        }

        let command = BuildSchemaCommand::new();
        let result = use_case.execute(command).await;

        assert!(result.is_ok(), "Failed with error: {:?}", result.err());
        let result = result.unwrap();
        assert_eq!(result.entity_count, 2);
        assert_eq!(result.action_count, 1);
    }

    #[tokio::test]
    async fn test_build_schema_empty_fails() {
        let use_case = create_use_case();

        let command = BuildSchemaCommand::new();
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        match result {
            Err(BuildSchemaError::EmptySchema) => {}
            _ => panic!("Expected EmptySchema error"),
        }
    }

    #[tokio::test]
    async fn test_build_schema_with_version() {
        let use_case = create_use_case();

        {
            let mut builder = use_case.builder().lock().unwrap();
            builder.register_entity::<MockUser>().unwrap();
        }

        let command = BuildSchemaCommand::new().with_version("v2.5.3");
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.version, Some("v2.5.3".to_string()));
    }

    #[tokio::test]
    async fn test_build_schema_without_validation() {
        let use_case = create_use_case();

        {
            let mut builder = use_case.builder().lock().unwrap();
            builder.register_entity::<MockUser>().unwrap();
        }

        let command = BuildSchemaCommand::new().with_validation(false);
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(!result.validated);
    }

    #[tokio::test]
    async fn test_build_schema_storage_failure() {
        let storage = MockSchemaStorage::with_failure();
        let use_case = create_use_case_with_storage(storage);

        {
            let mut builder = use_case.builder().lock().unwrap();
            builder.register_entity::<MockUser>().unwrap();
        }

        let command = BuildSchemaCommand::new();
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        match result {
            Err(BuildSchemaError::SchemaStorageError(_)) => {}
            _ => panic!("Expected SchemaStorageError"),
        }
    }

    #[tokio::test]
    async fn test_build_schema_resets_builder() {
        let use_case = create_use_case();

        // First build
        {
            let mut builder = use_case.builder().lock().unwrap();
            builder.register_entity::<MockUser>().unwrap();
        }

        let command = BuildSchemaCommand::new();
        let result1 = use_case.execute(command).await;
        assert!(result1.is_ok());

        // Builder should be reset now
        assert_eq!(use_case.entity_count().unwrap(), 0);
        assert_eq!(use_case.action_count().unwrap(), 0);

        // Second build should be able to register new types
        {
            let mut builder = use_case.builder().lock().unwrap();
            builder.register_entity::<MockDocument>().unwrap();
        }

        let command = BuildSchemaCommand::new();
        let result2 = use_case.execute(command).await;
        assert!(result2.is_ok());
        assert_eq!(result2.unwrap().entity_count, 1);
    }

    #[tokio::test]
    async fn test_entity_count_before_build() {
        let use_case = create_use_case();

        assert_eq!(use_case.entity_count().unwrap(), 0);

        {
            let mut builder = use_case.builder().lock().unwrap();
            builder.register_entity::<MockUser>().unwrap();
            builder.register_entity::<MockDocument>().unwrap();
        }

        assert_eq!(use_case.entity_count().unwrap(), 2);
    }

    #[tokio::test]
    async fn test_action_count_before_build() {
        let use_case = create_use_case();

        assert_eq!(use_case.action_count().unwrap(), 0);

        {
            let mut builder = use_case.builder().lock().unwrap();
            builder.register_action_type::<ReadAction>().unwrap();
        }

        assert_eq!(use_case.action_count().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_clear_registrations() {
        let use_case = create_use_case();

        {
            let mut builder = use_case.builder().lock().unwrap();
            builder.register_entity::<MockUser>().unwrap();
            builder.register_action_type::<ReadAction>().unwrap();
        }

        assert_eq!(use_case.entity_count().unwrap(), 1);
        assert_eq!(use_case.action_count().unwrap(), 1);

        let clear_result = use_case.clear();
        assert!(clear_result.is_ok());

        assert_eq!(use_case.entity_count().unwrap(), 0);
        assert_eq!(use_case.action_count().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_multiple_builds_with_storage() {
        let storage = Arc::new(MockSchemaStorage::new());
        let builder = Arc::new(Mutex::new(EngineBuilder::new()));
        let use_case = BuildSchemaUseCase::new(builder.clone(), storage.clone());

        // First build
        {
            let mut b = builder.lock().unwrap();
            b.register_entity::<MockUser>().unwrap();
        }
        use_case
            .execute(BuildSchemaCommand::new().with_version("v1.0.0"))
            .await
            .unwrap();

        // Second build
        {
            let mut b = builder.lock().unwrap();
            b.register_entity::<MockDocument>().unwrap();
        }
        use_case
            .execute(BuildSchemaCommand::new().with_version("v1.1.0"))
            .await
            .unwrap();

        // Storage should have both schemas
        assert_eq!(storage.get_saved_count(), 2);
    }

    #[tokio::test]
    async fn test_default_command() {
        let command = BuildSchemaCommand::default();
        assert!(command.version.is_none());
        assert!(command.validate);
    }

    #[tokio::test]
    async fn test_schema_stored_as_string() {
        let storage = Arc::new(MockSchemaStorage::new());
        let builder = Arc::new(Mutex::new(EngineBuilder::new()));
        let use_case = BuildSchemaUseCase::new(builder.clone(), storage.clone());

        {
            let mut b = builder.lock().unwrap();
            b.register_entity::<MockUser>().unwrap();
        }

        use_case.execute(BuildSchemaCommand::new()).await.unwrap();

        let last_saved = storage.get_last_saved();
        assert!(last_saved.is_some());
        let (schema_string, _) = last_saved.unwrap();
        // Verify it's a non-empty string (Cedar schema debug representation)
        assert!(!schema_string.is_empty());
        assert!(schema_string.len() > 10);
    }
}
