// crates/iam/src/domain/policy_test.rs

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::{Hrn, PolicyId};

    fn create_test_policy_id() -> PolicyId {
        PolicyId(Hrn::new("hrn:hodei:iam:global:org_123:policy/test_policy".to_string()))
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
    fn test_policy_update_content() {
        let policy_id = create_test_policy_id();
        let mut policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        let original_version = policy.metadata.version;
        let original_updated_at = policy.metadata.updated_at;

        let result = policy.update_content(
            "forbid(principal, action, resource);".to_string(),
            "user_456".to_string(),
        );

        assert!(result.is_ok());
        assert_eq!(policy.content, "forbid(principal, action, resource);");
        assert_eq!(policy.metadata.updated_by, "user_456");
        assert_eq!(policy.metadata.version, original_version + 1);
        assert!(policy.metadata.updated_at > original_updated_at);
    }

    #[test]
    fn test_policy_update_content_with_empty_content() {
        let policy_id = create_test_policy_id();
        let mut policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        let result = policy.update_content("".to_string(), "user_456".to_string());

        assert!(result.is_err());
        match result.unwrap_err() {
            PolicyError::InvalidContent(msg) => assert!(msg.contains("cannot be empty")),
            _ => panic!("Expected InvalidContent error"),
        }
    }

    #[test]
    fn test_policy_update_name() {
        let policy_id = create_test_policy_id();
        let mut policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        let result = policy.update_name("Updated Policy".to_string(), "user_456".to_string());

        assert!(result.is_ok());
        assert_eq!(policy.name, "Updated Policy");
        assert_eq!(policy.metadata.updated_by, "user_456");
    }

    #[test]
    fn test_policy_update_description() {
        let policy_id = create_test_policy_id();
        let mut policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        policy.update_description(Some("Test description".to_string()), "user_456".to_string());

        assert_eq!(policy.description, Some("Test description".to_string()));
        assert_eq!(policy.metadata.updated_by, "user_456");
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
    fn test_policy_activate_from_inactive() {
        let policy_id = create_test_policy_id();
        let mut policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        // First activate, then deactivate
        policy.activate("user_123".to_string()).unwrap();
        policy.deactivate("user_123".to_string()).unwrap();
        assert_eq!(policy.status, PolicyStatus::Inactive);

        // Now activate again
        let result = policy.activate("user_456".to_string());

        assert!(result.is_ok());
        assert_eq!(policy.status, PolicyStatus::Active);
        assert!(policy.is_active());
    }

    #[test]
    fn test_policy_activate_from_deprecated_fails() {
        let policy_id = create_test_policy_id();
        let mut policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        // Deprecate the policy
        policy.deprecate("user_123".to_string()).unwrap();
        assert_eq!(policy.status, PolicyStatus::Deprecated);

        // Try to activate - should fail
        let result = policy.activate("user_456".to_string());

        assert!(result.is_err());
        match result.unwrap_err() {
            PolicyError::InvalidStatusTransition { from, to } => {
                assert_eq!(from, PolicyStatus::Deprecated);
                assert_eq!(to, PolicyStatus::Active);
            }
            _ => panic!("Expected InvalidStatusTransition error"),
        }
    }

    #[test]
    fn test_policy_deactivate() {
        let policy_id = create_test_policy_id();
        let mut policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        // First activate
        policy.activate("user_123".to_string()).unwrap();
        assert_eq!(policy.status, PolicyStatus::Active);

        // Then deactivate
        let result = policy.deactivate("user_456".to_string());

        assert!(result.is_ok());
        assert_eq!(policy.status, PolicyStatus::Inactive);
        assert!(!policy.is_active());
        assert_eq!(policy.metadata.updated_by, "user_456");
    }

    #[test]
    fn test_policy_deprecate() {
        let policy_id = create_test_policy_id();
        let mut policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        let result = policy.deprecate("user_456".to_string());

        assert!(result.is_ok());
        assert_eq!(policy.status, PolicyStatus::Deprecated);
        assert!(!policy.is_active());
        assert!(!policy.can_be_modified());
        assert_eq!(policy.metadata.updated_by, "user_456");
    }

    #[test]
    fn test_policy_add_tag() {
        let policy_id = create_test_policy_id();
        let mut policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        policy.add_tag("engineering".to_string(), "user_456".to_string());

        assert_eq!(policy.metadata.tags, vec!["engineering"]);
        assert_eq!(policy.metadata.updated_by, "user_456");
    }

    #[test]
    fn test_policy_add_duplicate_tag() {
        let policy_id = create_test_policy_id();
        let mut policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        policy.add_tag("engineering".to_string(), "user_456".to_string());
        policy.add_tag("engineering".to_string(), "user_456".to_string());

        assert_eq!(policy.metadata.tags, vec!["engineering"]);
    }

    #[test]
    fn test_policy_remove_tag() {
        let policy_id = create_test_policy_id();
        let mut policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        policy.add_tag("engineering".to_string(), "user_123".to_string());
        policy.add_tag("security".to_string(), "user_123".to_string());
        assert_eq!(policy.metadata.tags, vec!["engineering", "security"]);

        policy.remove_tag("engineering", "user_456".to_string());

        assert_eq!(policy.metadata.tags, vec!["security"]);
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
        
        // Test same status transitions (should be allowed)
        for status in PolicyStatus::all() {
            assert!(status.can_transition_to(&status));
        }
        
        // Test some invalid transitions
        assert!(!PolicyStatus::Draft.can_transition_to(&PolicyStatus::Inactive));
        assert!(!PolicyStatus::Active.can_transition_to(&PolicyStatus::Draft));
        assert!(!PolicyStatus::Inactive.can_transition_to(&PolicyStatus::Draft));
    }

    #[test]
    fn test_policy_status_display() {
        assert_eq!(PolicyStatus::Draft.to_string(), "Draft");
        assert_eq!(PolicyStatus::Active.to_string(), "Active");
        assert_eq!(PolicyStatus::Inactive.to_string(), "Inactive");
        assert_eq!(PolicyStatus::Deprecated.to_string(), "Deprecated");
    }

    #[test]
    fn test_policy_error_display() {
        let error = PolicyError::InvalidStatusTransition {
            from: PolicyStatus::Deprecated,
            to: PolicyStatus::Active,
        };
        assert!(error.to_string().contains("Invalid status transition"));
        assert!(error.to_string().contains("Deprecated"));
        assert!(error.to_string().contains("Active"));

        let error = PolicyError::InvalidName("Test message".to_string());
        assert!(error.to_string().contains("Invalid policy name"));
        assert!(error.to_string().contains("Test message"));

        let error = PolicyError::InvalidContent("Test content error".to_string());
        assert!(error.to_string().contains("Invalid policy content"));
        assert!(error.to_string().contains("Test content error"));
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

    #[test]
    fn test_policy_clone() {
        let policy_id = create_test_policy_id();
        let policy = Policy::new(
            policy_id,
            "Test Policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            "user_123".to_string(),
        ).unwrap();

        let cloned_policy = policy.clone();
        assert_eq!(policy, cloned_policy);
    }
}