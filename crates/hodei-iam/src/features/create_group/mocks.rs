//! Mock implementations for testing
//!
//! This module provides mock implementations of the ports for use in unit tests.

use super::ports::CreateGroupPort;
use crate::internal::domain::Group;
use async_trait::async_trait;
use kernel::Hrn;
use kernel::HrnGenerator;
use std::sync::Arc;

/// Mock implementation of CreateGroupPort for testing
#[allow(dead_code)]
pub struct MockCreateGroupPort {
    /// Whether the save operation should fail
    pub should_fail: bool,
    /// The group that was saved (for inspection in tests)
    saved_group: Option<Group>,
}

#[async_trait]
impl CreateGroupPort for MockCreateGroupPort {
    async fn save_group(
        &self,
        _group_dto: &super::dto::GroupPersistenceDto,
    ) -> Result<(), super::error::CreateGroupError> {
        if self.should_fail {
            Err(super::error::CreateGroupError::PersistenceError(
                "Mock failure".to_string(),
            ))
        } else {
            // In a real mock, we might store the group for inspection
            // For this simple mock, we just return Ok
            Ok(())
        }
    }
}

#[allow(dead_code)]
impl MockCreateGroupPort {
    /// Create a new mock with default settings
    pub fn new() -> Self {
        Self {
            should_fail: false,
            saved_group: None,
        }
    }

    /// Create a new mock that will fail
    pub fn failing() -> Self {
        Self {
            should_fail: true,
            saved_group: None,
        }
    }
}

impl Default for MockCreateGroupPort {
    fn default() -> Self {
        Self::new()
    }
}

/// Mock implementation of HrnGenerator for testing
#[allow(dead_code)]
pub struct MockHrnGenerator {
    /// The HRN to return
    pub hrn: Hrn,
}

impl HrnGenerator for MockHrnGenerator {
    fn new_group_hrn(&self, _name: &str) -> Hrn {
        self.hrn.clone()
    }

    fn new_user_hrn(&self, _name: &str) -> Hrn {
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
pub fn create_default_mocks() -> (Arc<MockCreateGroupPort>, Arc<MockHrnGenerator>) {
    let hrn = Hrn::new(
        "hodei".to_string(),
        "iam".to_string(),
        "account123".to_string(),
        "Group".to_string(),
        "test-group".to_string(),
    );

    let persister = Arc::new(MockCreateGroupPort::new());
    let hrn_generator = Arc::new(MockHrnGenerator::new(hrn));

    (persister, hrn_generator)
}
