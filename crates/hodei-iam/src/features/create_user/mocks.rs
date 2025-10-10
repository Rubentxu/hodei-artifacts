//! Mock implementations for testing
//!
//! This module provides mock implementations of the ports for use in unit tests.

use super::dto::UserPersistenceDto;
use super::ports::CreateUserPort;
use async_trait::async_trait;
use kernel::Hrn;
use kernel::HrnGenerator;
use std::sync::Arc;

/// Mock implementation of CreateUserPort for testing
#[allow(dead_code)]
pub struct MockCreateUserPort {
    /// Whether the save operation should fail
    pub should_fail: bool,
    /// The user that was saved (for inspection in tests)
    pub saved_user_dto: Option<UserPersistenceDto>,
}

#[async_trait]
impl CreateUserPort for MockCreateUserPort {
    async fn save_user(
        &self,
        _user_dto: &super::dto::UserPersistenceDto,
    ) -> Result<(), super::error::CreateUserError> {
        if self.should_fail {
            Err(super::error::CreateUserError::PersistenceError(
                "Mock failure".to_string(),
            ))
        } else {
            // In a real mock, we might store the user for inspection
            // For this simple mock, we just return Ok
            Ok(())
        }
    }
}

#[allow(dead_code)]
impl MockCreateUserPort {
    /// Create a new mock with default settings
    pub fn new() -> Self {
        Self {
            should_fail: false,
            saved_user_dto: None,
        }
    }

    /// Create a new mock that will fail
    pub fn failing() -> Self {
        Self {
            should_fail: true,
            saved_user_dto: None,
        }
    }
}

/// Mock implementation of HrnGenerator for testing
#[allow(dead_code)]
pub struct MockHrnGenerator {
    /// The HRN to return
    pub hrn: Hrn,
}

impl HrnGenerator for MockHrnGenerator {
    fn new_user_hrn(&self, _name: &str) -> Hrn {
        self.hrn.clone()
    }

    fn new_group_hrn(&self, _name: &str) -> Hrn {
        self.hrn.clone()
    }
}

#[allow(dead_code)]
impl MockHrnGenerator {
    /// Create a new mock HRN generator
    pub fn new(hrn: Hrn) -> Self {
        Self { hrn }
    }
}

/// Create a set of default mocks for testing
#[allow(dead_code)]
pub fn create_default_mocks() -> (Arc<MockCreateUserPort>, Arc<MockHrnGenerator>) {
    let hrn = Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "account123".to_string(),
        "User".to_string(),
        "test-user".to_string(),
    );

    let persister = Arc::new(MockCreateUserPort::new());
    let hrn_generator = Arc::new(MockHrnGenerator::new(hrn));

    (persister, hrn_generator)
}
