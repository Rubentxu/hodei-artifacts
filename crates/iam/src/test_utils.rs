// crates/iam/src/test_utils.rs

#[cfg(test)]
use crate::domain::policy::{Policy, PolicyMetadata, PolicyStatus};
#[cfg(test)]
use shared::hrn::{Hrn, PolicyId};
#[cfg(test)]
use time::OffsetDateTime;

#[cfg(test)]
pub fn policy_id() -> PolicyId {
    PolicyId(Hrn::new("hrn:hodei:iam:global:org_123:policy/test_policy").expect("Valid HRN"))
}

#[cfg(test)]
pub fn create_test_policy() -> Policy {
    let now = OffsetDateTime::now_utc();
    Policy {
        id: policy_id(),
        name: "Test Policy".to_string(),
        description: Some("A test policy".to_string()),
        content: "permit(principal, action, resource);".to_string(),
        status: PolicyStatus::Active,
        metadata: PolicyMetadata {
            created_at: now,
            created_by: "test_user".to_string(),
            updated_at: now,
            updated_by: "test_user".to_string(),
            version: 1,
            tags: vec!["test".to_string()],
        },
    }
}

#[cfg(test)]
pub struct PolicyBuilder {
    policy: Policy,
}

#[cfg(test)]
impl PolicyBuilder {
    pub fn new() -> Self {
        Self {
            policy: create_test_policy(),
        }
    }

    pub fn with_id(mut self, id: PolicyId) -> Self {
        self.policy.id = id;
        self
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.policy.name = name;
        self
    }

    pub fn with_status(mut self, status: PolicyStatus) -> Self {
        self.policy.status = status;
        self
    }

    pub fn with_content(mut self, content: String) -> Self {
        self.policy.content = content;
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.policy.description = description;
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.policy.metadata.tags = tags;
        self
    }

    pub fn with_created_by(mut self, created_by: String) -> Self {
        self.policy.metadata.created_by = created_by;
        self
    }

    pub fn build(self) -> Policy {
        self.policy
    }
}

#[cfg(test)]
impl Default for PolicyBuilder {
    fn default() -> Self {
        Self::new()
    }
}
