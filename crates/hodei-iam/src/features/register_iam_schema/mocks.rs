//! Mock implementations for RegisterIamSchemaUseCase testing
//!
//! These mocks provide controlled test implementations for the ports used by
//! RegisterIamSchemaUseCase, enabling isolated unit testing.
//!
//! # Why the registrations are recorded
//!
//! The mocks used to answer "2 entity types, 6 action types" with hardcoded
//! literals, so every test passed no matter what the use case actually
//! registered — the false green that hid a completely broken registration
//! path. The registrar mocks now record what they were asked to register and
//! the build-schema mock reports *those* counts, so a registration that
//! silently does nothing shows up as a smaller count instead of a green test.

use async_trait::async_trait;
use hodei_policies::build_schema::dto::{BuildSchemaCommand, BuildSchemaResult};
use hodei_policies::build_schema::error::BuildSchemaError;
use hodei_policies::build_schema::ports::BuildSchemaPort;
use hodei_policies::register_action_type::dto::RegisterActionTypeCommand;
use hodei_policies::register_action_type::error::RegisterActionTypeError;
use hodei_policies::register_action_type::ports::RegisterActionTypePort;
use hodei_policies::register_entity_type::dto::RegisterEntityTypeCommand;
use hodei_policies::register_entity_type::error::RegisterEntityTypeError;
use hodei_policies::register_entity_type::ports::RegisterEntityTypePort;
use std::any::Any;
use std::sync::{Arc, Mutex};

/// What the registration mocks were actually asked to register.
///
/// The three mocks share one instance of this record, so the schema the
/// build-schema mock reports is the one the registrars were asked for.
#[derive(Debug, Default)]
pub struct RecordedRegistrations {
    /// The entity types that were requested, in registration order
    pub entity_types: Vec<String>,
    /// The action types that were requested, in registration order
    pub action_types: Vec<String>,
}

/// Mock implementation of RegisterEntityTypePort for testing
#[derive(Default)]
pub struct MockRegisterEntityTypePort {
    /// Whether the mock should simulate errors
    pub should_fail: bool,
    /// The registrations this mock has seen
    recorded: Arc<Mutex<RecordedRegistrations>>,
}

impl MockRegisterEntityTypePort {
    /// Create a new mock that succeeds
    pub fn new(recorded: Arc<Mutex<RecordedRegistrations>>) -> Self {
        Self {
            should_fail: false,
            recorded,
        }
    }

    /// Create a new mock that fails
    pub fn failing(recorded: Arc<Mutex<RecordedRegistrations>>) -> Self {
        Self {
            should_fail: true,
            recorded,
        }
    }

    /// The entity types this mock was asked to register
    pub fn registered_entity_types(&self) -> Vec<String> {
        self.recorded
            .lock()
            .map(|recorded| recorded.entity_types.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl RegisterEntityTypePort for MockRegisterEntityTypePort {
    async fn execute(
        &self,
        command: RegisterEntityTypeCommand,
    ) -> Result<(), RegisterEntityTypeError> {
        if self.should_fail {
            return Err(RegisterEntityTypeError::InternalError(
                "Mock entity registration failed".to_string(),
            ));
        }

        if let Ok(mut recorded) = self.recorded.lock() {
            recorded.entity_types.push(command.entity_type);
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Mock implementation of RegisterActionTypePort for testing
#[derive(Default)]
pub struct MockRegisterActionTypePort {
    /// Whether the mock should simulate errors
    pub should_fail: bool,
    /// The registrations this mock has seen
    recorded: Arc<Mutex<RecordedRegistrations>>,
}

impl MockRegisterActionTypePort {
    /// Create a new mock that succeeds
    pub fn new(recorded: Arc<Mutex<RecordedRegistrations>>) -> Self {
        Self {
            should_fail: false,
            recorded,
        }
    }

    /// Create a new mock that fails
    pub fn failing(recorded: Arc<Mutex<RecordedRegistrations>>) -> Self {
        Self {
            should_fail: true,
            recorded,
        }
    }

    /// The action types this mock was asked to register
    pub fn registered_action_types(&self) -> Vec<String> {
        self.recorded
            .lock()
            .map(|recorded| recorded.action_types.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl RegisterActionTypePort for MockRegisterActionTypePort {
    async fn execute(
        &self,
        command: RegisterActionTypeCommand,
    ) -> Result<(), RegisterActionTypeError> {
        if self.should_fail {
            return Err(RegisterActionTypeError::InternalError(
                "Mock action registration failed".to_string(),
            ));
        }

        if let Ok(mut recorded) = self.recorded.lock() {
            recorded.action_types.push(command.action_name);
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Mock implementation of BuildSchemaPort for testing
#[derive(Default)]
pub struct MockBuildSchemaPort {
    /// Whether the mock should simulate errors
    pub should_fail: bool,
    /// Version to return in successful builds
    pub version: Option<String>,
    /// Schema ID to return in successful builds
    pub schema_id: Option<String>,
    /// The registrations this mock reports as part of the built schema
    recorded: Arc<Mutex<RecordedRegistrations>>,
}

impl MockBuildSchemaPort {
    /// Create a new mock that succeeds
    pub fn new(recorded: Arc<Mutex<RecordedRegistrations>>) -> Self {
        Self {
            should_fail: false,
            version: None,
            schema_id: None,
            recorded,
        }
    }

    /// Create a new mock that fails
    pub fn failing(recorded: Arc<Mutex<RecordedRegistrations>>) -> Self {
        Self {
            should_fail: true,
            version: None,
            schema_id: None,
            recorded,
        }
    }

    /// Create a mock with specific version and schema ID
    pub fn with_version_and_id(
        recorded: Arc<Mutex<RecordedRegistrations>>,
        version: String,
        schema_id: String,
    ) -> Self {
        Self {
            should_fail: false,
            version: Some(version),
            schema_id: Some(schema_id),
            recorded,
        }
    }
}

#[async_trait]
impl BuildSchemaPort for MockBuildSchemaPort {
    async fn execute(
        &self,
        _command: BuildSchemaCommand,
    ) -> Result<BuildSchemaResult, BuildSchemaError> {
        if self.should_fail {
            return Err(BuildSchemaError::SchemaBuildError(
                "Mock schema build failed".to_string(),
            ));
        }

        let (entity_count, action_count) = self
            .recorded
            .lock()
            .map(|recorded| (recorded.entity_types.len(), recorded.action_types.len()))
            .unwrap_or((0, 0));

        Ok(BuildSchemaResult {
            version: self.version.clone(),
            schema_id: self
                .schema_id
                .clone()
                .unwrap_or_else(|| "test-schema-id".to_string()),
            validated: true,
            entity_count,
            action_count,
        })
    }
}

/// Helper function to create default mocks for testing
pub fn create_default_mocks() -> (
    Arc<MockRegisterEntityTypePort>,
    Arc<MockRegisterActionTypePort>,
    Arc<MockBuildSchemaPort>,
) {
    let recorded = Arc::new(Mutex::new(RecordedRegistrations::default()));

    (
        Arc::new(MockRegisterEntityTypePort::new(recorded.clone())),
        Arc::new(MockRegisterActionTypePort::new(recorded.clone())),
        Arc::new(MockBuildSchemaPort::new(recorded)),
    )
}

/// Helper function to create failing mocks for testing
pub fn create_failing_mocks() -> (
    Arc<MockRegisterEntityTypePort>,
    Arc<MockRegisterActionTypePort>,
    Arc<MockBuildSchemaPort>,
) {
    let recorded = Arc::new(Mutex::new(RecordedRegistrations::default()));

    (
        Arc::new(MockRegisterEntityTypePort::failing(recorded.clone())),
        Arc::new(MockRegisterActionTypePort::failing(recorded.clone())),
        Arc::new(MockBuildSchemaPort::failing(recorded)),
    )
}

/// Helper function to create mocks where only the entity registration fails
pub fn create_mocks_with_failing_entity() -> (
    Arc<MockRegisterEntityTypePort>,
    Arc<MockRegisterActionTypePort>,
    Arc<MockBuildSchemaPort>,
) {
    let recorded = Arc::new(Mutex::new(RecordedRegistrations::default()));

    (
        Arc::new(MockRegisterEntityTypePort::failing(recorded.clone())),
        Arc::new(MockRegisterActionTypePort::new(recorded.clone())),
        Arc::new(MockBuildSchemaPort::new(recorded)),
    )
}

/// Helper function to create mocks where only the action registration fails
pub fn create_mocks_with_failing_action() -> (
    Arc<MockRegisterEntityTypePort>,
    Arc<MockRegisterActionTypePort>,
    Arc<MockBuildSchemaPort>,
) {
    let recorded = Arc::new(Mutex::new(RecordedRegistrations::default()));

    (
        Arc::new(MockRegisterEntityTypePort::new(recorded.clone())),
        Arc::new(MockRegisterActionTypePort::failing(recorded.clone())),
        Arc::new(MockBuildSchemaPort::new(recorded)),
    )
}

/// Helper function to create mocks where only the schema build fails
pub fn create_mocks_with_failing_build() -> (
    Arc<MockRegisterEntityTypePort>,
    Arc<MockRegisterActionTypePort>,
    Arc<MockBuildSchemaPort>,
) {
    let recorded = Arc::new(Mutex::new(RecordedRegistrations::default()));

    (
        Arc::new(MockRegisterEntityTypePort::new(recorded.clone())),
        Arc::new(MockRegisterActionTypePort::new(recorded.clone())),
        Arc::new(MockBuildSchemaPort::failing(recorded)),
    )
}

/// Helper function to create mocks that report a specific schema version and ID
pub fn create_mocks_with_version_and_id(
    version: String,
    schema_id: String,
) -> (
    Arc<MockRegisterEntityTypePort>,
    Arc<MockRegisterActionTypePort>,
    Arc<MockBuildSchemaPort>,
) {
    let recorded = Arc::new(Mutex::new(RecordedRegistrations::default()));

    (
        Arc::new(MockRegisterEntityTypePort::new(recorded.clone())),
        Arc::new(MockRegisterActionTypePort::new(recorded.clone())),
        Arc::new(MockBuildSchemaPort::with_version_and_id(
            recorded, version, schema_id,
        )),
    )
}
