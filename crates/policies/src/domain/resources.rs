//! policies crate - Motor de políticas y autorización


use serde::{Deserialize, Serialize};
use shared::attributes::AttributeValue;
use std::collections::HashMap;

/// Representa un principal (usuario, servicio, etc.) en una solicitud de autorización
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
    pub entity_type: String,
    pub attributes: HashMap<String, AttributeValue>,
}

/// Representa un recurso que se intenta acceder en una solicitud de autorización
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    pub id: String,
    pub entity_type: String,
    pub attributes: HashMap<String, AttributeValue>,
}

/// Representa una acción realizada en una solicitud de autorización
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub name: String,
    pub attributes: HashMap<String, AttributeValue>,
}

/// Contexto adicional para una solicitud de autorización
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub attributes: HashMap<String, AttributeValue>,
}

/// Resultado de una decisión de autorización
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AuthorizationDecision {
    Allow,
    Deny,
}

// AttributeValue ahora se importa desde `shared::attributes`

impl Principal {
    pub fn new(id: impl Into<String>, entity_type: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            entity_type: entity_type.into(),
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }
}

impl Resource {
    pub fn new(id: impl Into<String>, entity_type: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            entity_type: entity_type.into(),
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }
}

impl Action {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }
}

impl Context {
    pub fn new() -> Self {
        Self {
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.attributes.insert(key.into(), value);
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
    fn basic_principal_resource_action_context() {
        let principal = Principal::new("alice", "User")
            .with_attribute("department", AttributeValue::String("engineering".into()))
            .with_attribute("level", AttributeValue::Long(5));

        let resource = Resource::new("artifact-123", "Artifact")
            .with_attribute("owner", AttributeValue::String("alice".into()))
            .with_attribute("public", AttributeValue::Boolean(false));

        let action = Action::new("read").with_attribute(
            "scope",
            AttributeValue::String("metadata".into()),
        );

        let context = Context::default().with_attribute(
            "ip_address",
            AttributeValue::String("127.0.0.1".into()),
        );

        assert_eq!(principal.entity_type, "User");
        assert_eq!(resource.entity_type, "Artifact");
        assert_eq!(action.name, "read");
        assert!(!context.attributes.is_empty());
    }
}
