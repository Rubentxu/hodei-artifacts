// crates/security/src/domain/authorization.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a principal (user, service, etc.) in an authorization request
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
    pub entity_type: String,
    pub attributes: HashMap<String, AttributeValue>,
}

/// Represents a resource being accessed in an authorization request
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    pub id: String,
    pub entity_type: String,
    pub attributes: HashMap<String, AttributeValue>,
}

/// Represents an action being performed in an authorization request
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub name: String,
    pub attributes: HashMap<String, AttributeValue>,
}

/// Represents additional context for an authorization request
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub attributes: HashMap<String, AttributeValue>,
}

/// The result of an authorization decision
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AuthorizationDecision {
    Allow,
    Deny,
}

/// Flexible attribute value type for Cedar policy attributes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttributeValue {
    String(String),
    Long(i64),
    Boolean(bool),
    Set(Vec<AttributeValue>),
    Record(HashMap<String, AttributeValue>),
}

impl Principal {
    pub fn new(id: String, entity_type: String) -> Self {
        Self {
            id,
            entity_type,
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: String, value: AttributeValue) -> Self {
        self.attributes.insert(key, value);
        self
    }
}

impl Resource {
    pub fn new(id: String, entity_type: String) -> Self {
        Self {
            id,
            entity_type,
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: String, value: AttributeValue) -> Self {
        self.attributes.insert(key, value);
        self
    }
}

impl Action {
    pub fn new(name: String) -> Self {
        Self {
            name,
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: String, value: AttributeValue) -> Self {
        self.attributes.insert(key, value);
        self
    }
}

impl Context {
    pub fn new() -> Self {
        Self {
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: String, value: AttributeValue) -> Self {
        self.attributes.insert(key, value);
        self
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_principal_creation() {
        let principal = Principal::new("alice".to_string(), "User".to_string());
        
        assert_eq!(principal.id, "alice");
        assert_eq!(principal.entity_type, "User");
        assert!(principal.attributes.is_empty());
    }

    #[test]
    fn test_principal_with_attributes() {
        let principal = Principal::new("alice".to_string(), "User".to_string())
            .with_attribute("department".to_string(), AttributeValue::String("engineering".to_string()))
            .with_attribute("level".to_string(), AttributeValue::Long(5));
        
        assert_eq!(principal.attributes.len(), 2);
        assert_eq!(
            principal.attributes.get("department"),
            Some(&AttributeValue::String("engineering".to_string()))
        );
        assert_eq!(
            principal.attributes.get("level"),
            Some(&AttributeValue::Long(5))
        );
    }

    #[test]
    fn test_resource_creation() {
        let resource = Resource::new("artifact-123".to_string(), "Artifact".to_string());
        
        assert_eq!(resource.id, "artifact-123");
        assert_eq!(resource.entity_type, "Artifact");
        assert!(resource.attributes.is_empty());
    }

    #[test]
    fn test_resource_with_attributes() {
        let resource = Resource::new("artifact-123".to_string(), "Artifact".to_string())
            .with_attribute("owner".to_string(), AttributeValue::String("alice".to_string()))
            .with_attribute("public".to_string(), AttributeValue::Boolean(false));
        
        assert_eq!(resource.attributes.len(), 2);
        assert_eq!(
            resource.attributes.get("owner"),
            Some(&AttributeValue::String("alice".to_string()))
        );
        assert_eq!(
            resource.attributes.get("public"),
            Some(&AttributeValue::Boolean(false))
        );
    }

    #[test]
    fn test_action_creation() {
        let action = Action::new("read".to_string());
        
        assert_eq!(action.name, "read");
        assert!(action.attributes.is_empty());
    }

    #[test]
    fn test_action_with_attributes() {
        let action = Action::new("read".to_string())
            .with_attribute("scope".to_string(), AttributeValue::String("metadata".to_string()));
        
        assert_eq!(action.attributes.len(), 1);
        assert_eq!(
            action.attributes.get("scope"),
            Some(&AttributeValue::String("metadata".to_string()))
        );
    }

    #[test]
    fn test_context_creation() {
        let context = Context::new();
        assert!(context.attributes.is_empty());
    }

    #[test]
    fn test_context_default() {
        let context = Context::default();
        assert!(context.attributes.is_empty());
    }

    #[test]
    fn test_context_with_attributes() {
        let context = Context::new()
            .with_attribute("time".to_string(), AttributeValue::String("2024-01-01T00:00:00Z".to_string()))
            .with_attribute("ip_address".to_string(), AttributeValue::String("192.168.1.1".to_string()));
        
        assert_eq!(context.attributes.len(), 2);
        assert_eq!(
            context.attributes.get("time"),
            Some(&AttributeValue::String("2024-01-01T00:00:00Z".to_string()))
        );
    }

    #[test]
    fn test_authorization_decision_variants() {
        let allow = AuthorizationDecision::Allow;
        let deny = AuthorizationDecision::Deny;
        
        assert_ne!(allow, deny);
        assert_eq!(allow, AuthorizationDecision::Allow);
        assert_eq!(deny, AuthorizationDecision::Deny);
    }

    #[test]
    fn test_attribute_value_variants() {
        let string_val = AttributeValue::String("test".to_string());
        let long_val = AttributeValue::Long(42);
        let bool_val = AttributeValue::Boolean(true);
        let set_val = AttributeValue::Set(vec![
            AttributeValue::String("item1".to_string()),
            AttributeValue::String("item2".to_string()),
        ]);
        let mut record_map = HashMap::new();
        record_map.insert("key".to_string(), AttributeValue::String("value".to_string()));
        let record_val = AttributeValue::Record(record_map);
        
        // Test equality
        assert_eq!(string_val, AttributeValue::String("test".to_string()));
        assert_eq!(long_val, AttributeValue::Long(42));
        assert_eq!(bool_val, AttributeValue::Boolean(true));
        
        // Test inequality
        assert_ne!(string_val, long_val);
        assert_ne!(bool_val, set_val);
        assert_ne!(set_val, record_val);
    }

    #[test]
    fn test_cloning_behavior() {
        let principal = Principal::new("alice".to_string(), "User".to_string())
            .with_attribute("dept".to_string(), AttributeValue::String("eng".to_string()));
        
        let cloned_principal = principal.clone();
        
        assert_eq!(principal.id, cloned_principal.id);
        assert_eq!(principal.entity_type, cloned_principal.entity_type);
        assert_eq!(principal.attributes, cloned_principal.attributes);
    }

    #[test]
    fn test_debug_formatting() {
        let principal = Principal::new("alice".to_string(), "User".to_string());
        let debug_str = format!("{:?}", principal);
        
        assert!(debug_str.contains("alice"));
        assert!(debug_str.contains("User"));
        assert!(debug_str.contains("Principal"));
    }
}