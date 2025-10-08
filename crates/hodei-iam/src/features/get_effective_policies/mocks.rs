//! Mock implementations for get_effective_policies ports
//!
//! These mocks are used exclusively for unit testing the use case.
//! They allow tests to control the behavior of external dependencies
//! without requiring real infrastructure (databases, services, etc.).

use async_trait::async_trait;

use crate::features::get_effective_policies::{
    dto::{GroupLookupDto, UserLookupDto},
    ports::{GroupFinderPort, PolicyFinderPort, UserFinderPort},
};
use kernel::domain::{HodeiPolicy, Hrn};

// Mock implementations for testing
#[derive(Debug, Clone)]
pub struct MockUserFinderPort {
    user: Option<UserLookupDto>,
    should_fail: bool,
}

impl MockUserFinderPort {
    pub fn new() -> Self {
        Self {
            user: None,
            should_fail: false,
        }
    }

    pub fn with_user(mut self, user: UserLookupDto) -> Self {
        self.user = Some(user);
        self
    }

    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

#[async_trait]
impl UserFinderPort for MockUserFinderPort {
    async fn find_by_hrn(
        &self,
        _hrn: &Hrn,
    ) -> Result<Option<UserLookupDto>, Box<dyn std::error::Error + Send + Sync>> {
        if self.should_fail {
            return Err("Mock user finder failure".into());
        }
        Ok(self.user.clone())
    }
}

#[derive(Debug, Clone)]
pub struct MockGroupFinderPort {
    groups: Vec<GroupLookupDto>,
    should_fail: bool,
}

impl MockGroupFinderPort {
    pub fn new() -> Self {
        Self {
            groups: vec![],
            should_fail: false,
        }
    }

    pub fn with_groups(mut self, groups: Vec<GroupLookupDto>) -> Self {
        self.groups = groups;
        self
    }

    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

#[async_trait]
impl GroupFinderPort for MockGroupFinderPort {
    async fn find_groups_by_user_hrn(
        &self,
        _user_hrn: &Hrn,
    ) -> Result<Vec<GroupLookupDto>, Box<dyn std::error::Error + Send + Sync>> {
        if self.should_fail {
            return Err("Mock group finder failure".into());
        }
        Ok(self.groups.clone())
    }
}

#[derive(Debug, Clone)]
pub struct MockPolicyFinderPort {
    policies: Vec<HodeiPolicy>,
    should_fail: bool,
}

impl MockPolicyFinderPort {
    pub fn new() -> Self {
        Self {
            policies: vec![],
            should_fail: false,
        }
    }

    pub fn with_policies(mut self, policies: Vec<HodeiPolicy>) -> Self {
        self.policies = policies;
        self
    }

    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

#[async_trait]
impl PolicyFinderPort for MockPolicyFinderPort {
    async fn find_policies_by_principal(
        &self,
        _principal_hrn: &Hrn,
    ) -> Result<Vec<HodeiPolicy>, Box<dyn std::error::Error + Send + Sync>> {
        if self.should_fail {
            return Err("Mock policy finder failure".into());
        }
        Ok(self.policies.clone())
    }
}
