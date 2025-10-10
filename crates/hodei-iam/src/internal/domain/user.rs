//! User entity - implements kernel traits for integration with hodei-policies

use kernel::Hrn;
use kernel::domain::entity::{HodeiEntity, HodeiEntityType, Principal, Resource};
use kernel::domain::value_objects::{ResourceTypeName, ServiceName};
use kernel::{AttributeName, AttributeType, AttributeValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// User entity representing an IAM user identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct User {
    /// Hierarchical Resource Name (unique identifier)
    pub hrn: Hrn,
    /// User's display name
    pub name: String,
    /// User's email address
    pub email: String,
    /// HRNs of groups this user belongs to
    pub group_hrns: Vec<Hrn>,
    /// Tags for categorization
    pub tags: Vec<String>,
}

#[allow(dead_code)]
impl User {
    /// Create a new user
    pub(crate) fn new(hrn: Hrn, name: String, email: String) -> Self {
        Self {
            hrn,
            name,
            email,
            group_hrns: Vec::new(),
            tags: Vec::new(),
        }
    }

    /// Add user to a group (idempotent)
    pub(crate) fn add_to_group(&mut self, group_hrn: Hrn) {
        if !self.group_hrns.contains(&group_hrn) {
            self.group_hrns.push(group_hrn);
        }
    }

    /// Remove user from a group
    pub(crate) fn remove_from_group(&mut self, group_hrn: &Hrn) {
        self.group_hrns.retain(|hrn| hrn != group_hrn);
    }

    /// Get all groups this user belongs to
    pub(crate) fn groups(&self) -> &[Hrn] {
        &self.group_hrns
    }

    /// Get user's email
    pub(crate) fn email(&self) -> &str {
        &self.email
    }
}

// ============================================================================
// Kernel Traits Implementation
// ============================================================================

impl HodeiEntityType for User {
    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn resource_type_name() -> ResourceTypeName {
        ResourceTypeName::new("User").expect("Valid resource type")
    }

    fn is_principal_type() -> bool {
        true
    }

    fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
        vec![
            (
                AttributeName::new("name").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("email").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("tags").expect("Valid attribute name"),
                AttributeType::set(AttributeType::string()),
            ),
        ]
    }
}

impl HodeiEntity for User {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
        let mut attrs = HashMap::new();

        attrs.insert(
            AttributeName::new("name").expect("Valid attribute name"),
            AttributeValue::string(&self.name),
        );

        attrs.insert(
            AttributeName::new("email").expect("Valid attribute name"),
            AttributeValue::string(&self.email),
        );

        let tag_values: Vec<AttributeValue> =
            self.tags.iter().map(AttributeValue::string).collect();
        attrs.insert(
            AttributeName::new("tags").expect("Valid attribute name"),
            AttributeValue::set(tag_values),
        );

        attrs
    }

    fn parent_hrns(&self) -> Vec<Hrn> {
        self.group_hrns.clone()
    }
}

// User can act as both Principal (for authorization) and Resource (for policies about users)
impl Principal for User {}
impl Resource for User {}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::AttributeName;

    #[test]
    fn test_user_creation() {
        let hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let user = User::new(
            hrn.clone(),
            "Alice".to_string(),
            "alice@example.com".to_string(),
        );

        assert_eq!(user.hrn, hrn);
        assert_eq!(user.name, "Alice");
        assert_eq!(user.email, "alice@example.com");
        assert!(user.group_hrns.is_empty());
        assert!(user.tags.is_empty());
    }

    #[test]
    fn test_add_user_to_group() {
        let user_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let group_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "admins".to_string(),
        );

        let mut user = User::new(
            user_hrn,
            "Alice".to_string(),
            "alice@example.com".to_string(),
        );
        user.add_to_group(group_hrn.clone());

        assert_eq!(user.group_hrns.len(), 1);
        assert_eq!(user.group_hrns[0], group_hrn);
    }

    #[test]
    fn test_add_to_group_is_idempotent() {
        let user_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let group_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "admins".to_string(),
        );

        let mut user = User::new(
            user_hrn,
            "Alice".to_string(),
            "alice@example.com".to_string(),
        );
        user.add_to_group(group_hrn.clone());
        user.add_to_group(group_hrn.clone()); // Add again

        assert_eq!(user.group_hrns.len(), 1);
        assert_eq!(user.group_hrns[0], group_hrn);
    }

    #[test]
    fn test_remove_from_group() {
        let user_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let group_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "admins".to_string(),
        );

        let mut user = User::new(
            user_hrn,
            "Alice".to_string(),
            "alice@example.com".to_string(),
        );
        user.add_to_group(group_hrn.clone());
        assert_eq!(user.group_hrns.len(), 1);

        user.remove_from_group(&group_hrn);
        assert_eq!(user.group_hrns.len(), 0);
    }

    #[test]
    fn test_user_implements_hodei_entity() {
        let hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let user = User::new(
            hrn.clone(),
            "Alice".to_string(),
            "alice@example.com".to_string(),
        );

        // Test HodeiEntity trait methods
        assert_eq!(user.hrn(), &hrn);
        assert_eq!(user.parent_hrns().len(), 0); // No groups yet

        let attrs = user.attributes();
        assert_eq!(attrs.len(), 3);
        assert_eq!(
            attrs.get(&AttributeName::new("name").expect("valid")),
            Some(&AttributeValue::string("Alice"))
        );
        assert_eq!(
            attrs.get(&AttributeName::new("email").expect("valid")),
            Some(&AttributeValue::string("alice@example.com"))
        );
    }

    #[test]
    fn test_user_entity_type_metadata() {
        assert_eq!(User::service_name().as_str(), "iam");
        assert_eq!(User::resource_type_name().as_str(), "User");
        assert!(User::is_principal_type());
        assert!(User::is_resource_type());

        let schema = User::attributes_schema();
        assert_eq!(schema.len(), 3);
    }
}
