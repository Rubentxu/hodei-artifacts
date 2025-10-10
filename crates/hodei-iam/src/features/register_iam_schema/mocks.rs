//! Mock implementations for RegisterIamSchemaUseCase testing
//!
//! These mocks provide controlled test implementations for the ports used by
//! RegisterIamSchemaUseCase, enabling isolated unit testing.

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
use std::sync::Arc;

/// Mock implementation of RegisterEntityTypePort for testing
#[derive(Default)]
pub struct MockRegisterEntityTypePort {
    /// Whether the mock should simulate errors
    pub should_fail: bool,
    /// Number of times execute was called
    #[allow(dead_code)]
    pub execute_calls: usize,
}

impl MockRegisterEntityTypePort {
    /// Create a new mock that succeeds
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new mock that fails
    pub fn failing() -> Self {
        Self {
            should_fail: true,
            ..Default::default()
        }
    }
}

#[async_trait]
impl RegisterEntityTypePort for MockRegisterEntityTypePort {
    async fn execute(
        &self,
        _command: RegisterEntityTypeCommand,
    ) -> Result<(), RegisterEntityTypeError> {
        if self.should_fail {
            Err(RegisterEntityTypeError::InternalError(
                "Mock entity registration failed".to_string(),
            ))
        } else {
            Ok(())
        }
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
    /// Number of times execute was called
    #[allow(dead_code)]
    pub execute_calls: usize,
}

impl MockRegisterActionTypePort {
    /// Create a new mock that succeeds
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new mock that fails
    pub fn failing() -> Self {
        Self {
            should_fail: true,
            ..Default::default()
        }
    }
}

#[async_trait]
impl RegisterActionTypePort for MockRegisterActionTypePort {
    async fn execute(
        &self,
        _command: RegisterActionTypeCommand,
    ) -> Result<(), RegisterActionTypeError> {
        if self.should_fail {
            Err(RegisterActionTypeError::InternalError(
                "Mock action registration failed".to_string(),
            ))
        } else {
            Ok(())
        }
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
    /// Number of times execute was called
    #[allow(dead_code)]
    pub execute_calls: usize,
}

impl MockBuildSchemaPort {
    /// Create a new mock that succeeds
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new mock that fails
    pub fn failing() -> Self {
        Self {
            should_fail: true,
            ..Default::default()
        }
    }

    /// Create a mock with specific version and schema ID
    pub fn with_version_and_id(version: String, schema_id: String) -> Self {
        Self {
            version: Some(version),
            schema_id: Some(schema_id),
            ..Default::default()
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
            Err(BuildSchemaError::SchemaBuildError(
                "Mock schema build failed".to_string(),
            ))
        } else {
            Ok(BuildSchemaResult {
                version: self.version.clone(),
                schema_id: self
                    .schema_id
                    .clone()
                    .unwrap_or_else(|| "test-schema-id".to_string()),
                validated: true,
                entity_count: 2,
                action_count: 6,
            })
        }
    }
}

/// Helper function to create default mocks for testing
pub fn create_default_mocks() -> (
    Arc<MockRegisterEntityTypePort>,
    Arc<MockRegisterActionTypePort>,
    Arc<MockBuildSchemaPort>,
) {
    (
        Arc::new(MockRegisterEntityTypePort::new()),
        Arc::new(MockRegisterActionTypePort::new()),
        Arc::new(MockBuildSchemaPort::new()),
    )
}

/// Helper function to create failing mocks for testing
pub fn create_failing_mocks() -> (
    Arc<MockRegisterEntityTypePort>,
    Arc<MockRegisterActionTypePort>,
    Arc<MockBuildSchemaPort>,
) {
    (
        Arc::new(MockRegisterEntityTypePort::failing()),
        Arc::new(MockRegisterActionTypePort::failing()),
        Arc::new(MockBuildSchemaPort::failing()),
    )
}
