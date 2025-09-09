// crates/iam/src/domain/policy.rs

use serde::{Deserialize, Serialize};
use shared::hrn::PolicyId;
use time::OffsetDateTime;

/// Represents a Cedar policy in the IAM system
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub content: String, // Cedar DSL content
    pub status: PolicyStatus,
    pub metadata: PolicyMetadata,
}

/// Policy lifecycle status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PolicyStatus {
    Draft,
    Active,
    Inactive,
    Deprecated,
}

/// Policy metadata for tracking creation and updates
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyMetadata {
    pub created_at: OffsetDateTime,
    pub created_by: String, // User ID
    pub updated_at: OffsetDateTime,
    pub updated_by: String, // User ID
    pub version: u32,
    pub tags: Vec<String>,
}

/// Custom error type for policy domain operations
#[derive(Debug, Clone, PartialEq)]
pub enum PolicyError {
    InvalidStatusTransition {
        from: PolicyStatus,
        to: PolicyStatus,
    },
    InvalidName(String),
    InvalidContent(String),
    InvalidVersion,
}

impl Policy {
    /// Create a new policy in Draft status
    pub fn new(
        id: PolicyId,
        name: String,
        content: String,
        created_by: String,
    ) -> Result<Self, PolicyError> {
        // Validate name
        if name.trim().is_empty() {
            return Err(PolicyError::InvalidName("Policy name cannot be empty".to_string()));
        }
        
        if name.len() > 255 {
            return Err(PolicyError::InvalidName("Policy name too long (max 255 characters)".to_string()));
        }

        // Validate content
        if content.trim().is_empty() {
            return Err(PolicyError::InvalidContent("Policy content cannot be empty".to_string()));
        }

        let now = OffsetDateTime::now_utc();
        Ok(Self {
            id,
            name,
            description: None,
            content,
            status: PolicyStatus::Draft,
            metadata: PolicyMetadata {
                created_at: now,
                created_by: created_by.clone(),
                updated_at: now,
                updated_by: created_by,
                version: 1,
                tags: Vec::new(),
            },
        })
    }

    /// Update policy content and increment version
    pub fn update_content(&mut self, content: String, updated_by: String) -> Result<(), PolicyError> {
        if content.trim().is_empty() {
            return Err(PolicyError::InvalidContent("Policy content cannot be empty".to_string()));
        }

        self.content = content;
        self.metadata.updated_at = OffsetDateTime::now_utc();
        self.metadata.updated_by = updated_by;
        self.metadata.version += 1;
        Ok(())
    }

    /// Update policy name
    pub fn update_name(&mut self, name: String, updated_by: String) -> Result<(), PolicyError> {
        if name.trim().is_empty() {
            return Err(PolicyError::InvalidName("Policy name cannot be empty".to_string()));
        }
        
        if name.len() > 255 {
            return Err(PolicyError::InvalidName("Policy name too long (max 255 characters)".to_string()));
        }

        self.name = name;
        self.metadata.updated_at = OffsetDateTime::now_utc();
        self.metadata.updated_by = updated_by;
        Ok(())
    }

    /// Update policy description
    pub fn update_description(&mut self, description: Option<String>, updated_by: String) {
        self.description = description;
        self.metadata.updated_at = OffsetDateTime::now_utc();
        self.metadata.updated_by = updated_by;
    }

    /// Activate a policy (from Draft or Inactive)
    pub fn activate(&mut self, updated_by: String) -> Result<(), PolicyError> {
        match self.status {
            PolicyStatus::Draft | PolicyStatus::Inactive => {
                self.status = PolicyStatus::Active;
                self.metadata.updated_at = OffsetDateTime::now_utc();
                self.metadata.updated_by = updated_by;
                Ok(())
            }
            PolicyStatus::Active => Ok(()), // Already active, no-op
            PolicyStatus::Deprecated => Err(PolicyError::InvalidStatusTransition {
                from: PolicyStatus::Deprecated,
                to: PolicyStatus::Active,
            }),
        }
    }

    /// Deactivate a policy (from Active to Inactive)
    pub fn deactivate(&mut self, updated_by: String) -> Result<(), PolicyError> {
        match self.status {
            PolicyStatus::Active => {
                self.status = PolicyStatus::Inactive;
                self.metadata.updated_at = OffsetDateTime::now_utc();
                self.metadata.updated_by = updated_by;
                Ok(())
            }
            PolicyStatus::Inactive => Ok(()), // Already inactive, no-op
            PolicyStatus::Draft | PolicyStatus::Deprecated => Err(PolicyError::InvalidStatusTransition {
                from: self.status.clone(),
                to: PolicyStatus::Inactive,
            }),
        }
    }

    /// Deprecate a policy (from any status except already deprecated)
    pub fn deprecate(&mut self, updated_by: String) -> Result<(), PolicyError> {
        match self.status {
            PolicyStatus::Deprecated => Ok(()), // Already deprecated, no-op
            _ => {
                self.status = PolicyStatus::Deprecated;
                self.metadata.updated_at = OffsetDateTime::now_utc();
                self.metadata.updated_by = updated_by;
                Ok(())
            }
        }
    }

    /// Add a tag to the policy
    pub fn add_tag(&mut self, tag: String, updated_by: String) {
        if !self.metadata.tags.contains(&tag) {
            self.metadata.tags.push(tag);
            self.metadata.updated_at = OffsetDateTime::now_utc();
            self.metadata.updated_by = updated_by;
        }
    }

    /// Remove a tag from the policy
    pub fn remove_tag(&mut self, tag: &str, updated_by: String) {
        if let Some(pos) = self.metadata.tags.iter().position(|t| t == tag) {
            self.metadata.tags.remove(pos);
            self.metadata.updated_at = OffsetDateTime::now_utc();
            self.metadata.updated_by = updated_by;
        }
    }

    /// Check if policy is active and can be used for authorization
    pub fn is_active(&self) -> bool {
        matches!(self.status, PolicyStatus::Active)
    }

    /// Check if policy can be modified
    pub fn can_be_modified(&self) -> bool {
        !matches!(self.status, PolicyStatus::Deprecated)
    }
}

impl PolicyStatus {
    /// Get all possible status values
    pub fn all() -> Vec<PolicyStatus> {
        vec![
            PolicyStatus::Draft,
            PolicyStatus::Active,
            PolicyStatus::Inactive,
            PolicyStatus::Deprecated,
        ]
    }

    /// Check if transition to another status is valid
    pub fn can_transition_to(&self, target: &PolicyStatus) -> bool {
        match (self, target) {
            // From Draft
            (PolicyStatus::Draft, PolicyStatus::Active) => true,
            (PolicyStatus::Draft, PolicyStatus::Deprecated) => true,
            
            // From Active
            (PolicyStatus::Active, PolicyStatus::Inactive) => true,
            (PolicyStatus::Active, PolicyStatus::Deprecated) => true,
            
            // From Inactive
            (PolicyStatus::Inactive, PolicyStatus::Active) => true,
            (PolicyStatus::Inactive, PolicyStatus::Deprecated) => true,
            
            // From Deprecated (no transitions allowed)
            (PolicyStatus::Deprecated, _) => false,
            
            // Same status (no-op)
            (a, b) if a == b => true,
            
            // All other transitions are invalid
            _ => false,
        }
    }
}

impl std::fmt::Display for PolicyStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyStatus::Draft => write!(f, "Draft"),
            PolicyStatus::Active => write!(f, "Active"),
            PolicyStatus::Inactive => write!(f, "Inactive"),
            PolicyStatus::Deprecated => write!(f, "Deprecated"),
        }
    }
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::InvalidStatusTransition { from, to } => {
                write!(f, "Invalid status transition from {} to {}", from, to)
            }
            PolicyError::InvalidName(msg) => write!(f, "Invalid policy name: {}", msg),
            PolicyError::InvalidContent(msg) => write!(f, "Invalid policy content: {}", msg),
            PolicyError::InvalidVersion => write!(f, "Invalid policy version"),
        }
    }
}

impl std::error::Error for PolicyError {}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::{Hrn, PolicyId};

    fn create_test_policy_id() -> PolicyId {
        PolicyId(Hrn::new("hrn:hodei:iam:global:org_123:policy/test_policy").expect("Valid HRN"))
    }

    #[test]
    fn test_policy_creation() {
        let policy_id = create_test_policy_id();
        let policy = Policy::new(
            policy_id.clone(),
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        );

        assert!(policy.is_ok());
        let policy = policy.unwrap();
        
        assert_eq!(policy.id, policy_id);
        assert_eq!(policy.name, "Test Policy");
        assert_eq!(policy.content, "permit(principal, action, resource);");
        assert_eq!(policy.status, PolicyStatus::Draft);
        assert_eq!(policy.metadata.created_by, "user_123");
        assert_eq!(policy.metadata.updated_by, "user_123");
        assert_eq!(policy.metadata.version, 1);
        assert!(policy.metadata.tags.is_empty());
        assert!(policy.description.is_none());
    }

    #[test]
    fn test_policy_creation_with_empty_name() {
        let policy_id = create_test_policy_id();
        let result = Policy::new(
            policy_id,
            "".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        );

        assert!(result.is_err());
        match result.unwrap_err() {
            PolicyError::InvalidName(msg) => assert!(msg.contains("cannot be empty")),
            _ => panic!("Expected InvalidName error"),
        }
    }

    #[test]
    fn test_policy_creation_with_long_name() {
        let policy_id = create_test_policy_id();
        let long_name = "a".repeat(256);
        let result = Policy::new(
            policy_id,
            long_name,
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        );

        assert!(result.is_err());
        match result.unwrap_err() {
            PolicyError::InvalidName(msg) => assert!(msg.contains("too long")),
            _ => panic!("Expected InvalidName error"),
        }
    }

    #[test]
    fn test_policy_creation_with_empty_content() {
        let policy_id = create_test_policy_id();
        let result = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "".to_string(),
            "user_123".to_string(),
        );

        assert!(result.is_err());
        match result.unwrap_err() {
            PolicyError::InvalidContent(msg) => assert!(msg.contains("cannot be empty")),
            _ => panic!("Expected InvalidContent error"),
        }
    }

    #[test]
    fn test_policy_activate_from_draft() {
        let policy_id = create_test_policy_id();
        let mut policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        assert_eq!(policy.status, PolicyStatus::Draft);
        assert!(!policy.is_active());

        let result = policy.activate("user_456".to_string());

        assert!(result.is_ok());
        assert_eq!(policy.status, PolicyStatus::Active);
        assert!(policy.is_active());
        assert_eq!(policy.metadata.updated_by, "user_456");
    }

    #[test]
    fn test_policy_status_transitions() {
        // Test all valid transitions
        assert!(PolicyStatus::Draft.can_transition_to(&PolicyStatus::Active));
        assert!(PolicyStatus::Draft.can_transition_to(&PolicyStatus::Deprecated));
        
        assert!(PolicyStatus::Active.can_transition_to(&PolicyStatus::Inactive));
        assert!(PolicyStatus::Active.can_transition_to(&PolicyStatus::Deprecated));
        
        assert!(PolicyStatus::Inactive.can_transition_to(&PolicyStatus::Active));
        assert!(PolicyStatus::Inactive.can_transition_to(&PolicyStatus::Deprecated));
        
        // Test invalid transitions from Deprecated
        assert!(!PolicyStatus::Deprecated.can_transition_to(&PolicyStatus::Draft));
        assert!(!PolicyStatus::Deprecated.can_transition_to(&PolicyStatus::Active));
        assert!(!PolicyStatus::Deprecated.can_transition_to(&PolicyStatus::Inactive));
    }

    #[test]
    fn test_policy_serialization() {
        let policy_id = create_test_policy_id();
        let policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        // Test serialization to JSON
        let json = serde_json::to_string(&policy).expect("Should serialize to JSON");
        assert!(json.contains("Test Policy"));
        assert!(json.contains("permit(principal, action, resource);"));
        assert!(json.contains("Draft"));

        // Test deserialization from JSON
        let deserialized: Policy = serde_json::from_str(&json).expect("Should deserialize from JSON");
        assert_eq!(deserialized, policy);
    }
}