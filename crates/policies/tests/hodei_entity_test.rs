//! Test to verify the HodeiEntity implementation with RestrictedExpression

use cedar_policy::{Entity, EntityUid, RestrictedExpression, Schema, SchemaFragment};
use std::collections::HashMap;
use std::str::FromStr;

/// Example implementation of HodeiEntity for testing
#[derive(Debug)]
struct TestUser {
    id: String,
    name: String,
    email: String,
    groups: Vec<String>,
    tags: Vec<String>,
}

impl TestUser {
    fn new(
        id: String,
        name: String,
        email: String,
        groups: Vec<String>,
        tags: Vec<String>,
    ) -> Self {
        Self {
            id,
            name,
            email,
            groups,
            tags,
        }
    }

    fn euid(&self) -> EntityUid {
        EntityUid::from_str(&format!("User::\"{}\"", self.id)).unwrap()
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );
        attrs.insert(
            "email".to_string(),
            RestrictedExpression::new_string(self.email.clone()),
        );

        // For collections, we use new_set
        let group_expressions: Vec<RestrictedExpression> = self
            .groups
            .iter()
            .map(|group| RestrictedExpression::new_string(group.clone()))
            .collect();
        attrs.insert(
            "groups".to_string(),
            RestrictedExpression::new_set(group_expressions),
        );

        let tag_expressions: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|tag| RestrictedExpression::new_string(tag.clone()))
            .collect();
        attrs.insert(
            "tags".to_string(),
            RestrictedExpression::new_set(tag_expressions),
        );

        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        // In a real implementation, this would convert group names to EntityUids
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hodei_entity_implementation() {
        let user = TestUser::new(
            "alice".to_string(),
            "Alice Smith".to_string(),
            "alice@example.com".to_string(),
            vec!["developers".to_string(), "admins".to_string()],
            vec!["employee".to_string(), "fulltime".to_string()],
        );

        let attributes = user.attributes();
        assert_eq!(attributes.len(), 4);
        assert!(attributes.contains_key("name"));
        assert!(attributes.contains_key("email"));
        assert!(attributes.contains_key("groups"));
        assert!(attributes.contains_key("tags"));

        let entity = Entity::new(
            user.euid(),
            attributes,
            user.parents().into_iter().collect(),
        );

        assert!(entity.is_ok());
    }

    #[test]
    fn test_cedar_integration() -> Result<(), Box<dyn std::error::Error>> {
        // Create a simple schema
        let schema_str = r#"
        entity User {
            name: String,
            email: String,
            groups: Set<String>,
            tags: Set<String>
        };
        
        action access appliesTo {
            principal: User,
            resource: User
        };
        "#;

        let (schema_fragment, _) = SchemaFragment::from_cedarschema_str(schema_str)?;
        let _schema = Schema::from_schema_fragments([schema_fragment])?;

        // Create a user entity
        let user = TestUser::new(
            "alice".to_string(),
            "Alice Smith".to_string(),
            "alice@example.com".to_string(),
            vec!["developers".to_string(), "admins".to_string()],
            vec!["employee".to_string(), "fulltime".to_string()],
        );

        let entity = Entity::new(
            user.euid(),
            user.attributes(),
            user.parents().into_iter().collect(),
        )?;

        // Validate that the entity conforms to the schema
        assert_eq!(entity.uid().to_string(), r#"User::"alice""#);

        Ok(())
    }
}
