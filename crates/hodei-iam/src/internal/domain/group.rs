//! Group entity - implements kernel traits for integration with hodei-policies

use kernel::Hrn;
use kernel::domain::entity::{HodeiEntity, HodeiEntityType, Resource};
use kernel::domain::value_objects::{ResourceTypeName, ServiceName};
use kernel::{AttributeName, AttributeType, AttributeValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Group entity representing an IAM group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Group {
    /// Hierarchical Resource Name (unique identifier)
    pub hrn: Hrn,
    /// Group's display name
    pub name: String,
    /// Optional descriptionº
    pub description: Option<String>,
    /// Tags for categorization
    pub tags: Vec<String>,
}

impl Group {
    /// Create a new group
    pub(crate) fn new(hrn: Hrn, name: String, description: Option<String>) -> Self {
        Self {
            hrn,
            name,
            description,
            tags: Vec::new(),
        }
    }
}

// ============================================================================
// Kernel Traits Implementation
// ============================================================================

impl HodeiEntityType for Group {
    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn resource_type_name() -> ResourceTypeName {
        ResourceTypeName::new("Group").expect("Valid resource type")
    }

    fn is_principal_type() -> bool {
        false // Groups are not principals, users are
    }

    fn is_resource_type() -> bool {
        true
    }

    fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
        vec![
            (
                AttributeName::new("name").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("description").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("tags").expect("Valid attribute name"),
                AttributeType::set(AttributeType::string()),
            ),
        ]
    }
}

impl HodeiEntity for Group {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
        let mut attrs = HashMap::new();

        attrs.insert(
            AttributeName::new("name").expect("Valid attribute name"),
            AttributeValue::string(&self.name),
        );

        if let Some(desc) = &self.description {
            attrs.insert(
                AttributeName::new("description").expect("Valid attribute name"),
                AttributeValue::string(desc),
            );
        }

        let tag_values: Vec<AttributeValue> =
            self.tags.iter().map(AttributeValue::string).collect();
        attrs.insert(
            AttributeName::new("tags").expect("Valid attribute name"),
            AttributeValue::set(tag_values),
        );

        attrs
    }

    fn parent_hrns(&self) -> Vec<Hrn> {
        // Groups don't have parents in this implementation
        Vec::new()
    }
}

// Group is a Resource (policies can be about groups)
// Groups are NOT principals (they don't act, users do)
impl Resource for Group {}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::AttributeName;

    #[test]
    fn test_group_creation() {
        let hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "admins".to_string(),
        );

        let group = Group::new(
            hrn.clone(),
            "Admins".to_string(),
            Some("Administrator group".to_string()),
        );

        assert_eq!(group.hrn, hrn);
        assert_eq!(group.name, "Admins");
        assert_eq!(group.description, Some("Administrator group".to_string()));
        assert!(group.tags.is_empty());
    }

    #[test]
    fn test_group_without_description() {
        let hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "users".to_string(),
        );

        let group = Group::new(hrn.clone(), "Users".to_string(), None);

        assert_eq!(group.hrn, hrn);
        assert_eq!(group.name, "Users");
        assert_eq!(group.description, None);
        assert!(group.tags.is_empty());
    }

    #[test]
    fn test_group_implements_hodei_entity() {
        let hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Group".to_string(),
            "admins".to_string(),
        );

        let group = Group::new(
            hrn.clone(),
            "Admins".to_string(),
            Some("Administrator group".to_string()),
        );

        // Test HodeiEntity trait methods
        assert_eq!(group.hrn(), &hrn);
        assert_eq!(group.parent_hrns().len(), 0);

        let attrs = group.attributes();
        assert!(attrs.len() >= 2);
        assert_eq!(
            attrs.get(&AttributeName::new("name").expect("valid")),
            Some(&AttributeValue::string("Admins"))
        );
        assert_eq!(
            attrs.get(&AttributeName::new("description").expect("valid")),
            Some(&AttributeValue::string("Administrator group"))
        );
    }

    #[test]
    fn test_group_entity_type_metadata() {
        assert_eq!(Group::service_name().as_str(), "iam");
        assert_eq!(Group::resource_type_name().as_str(), "Group");
        assert!(!Group::is_principal_type());
        assert!(Group::is_resource_type());

        let schema = Group::attributes_schema();
        assert_eq!(schema.len(), 3);
    }
}
