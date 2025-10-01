//! Test to verify schema implementation works correctly

use cedar_policy::{
    Context, Entities, Entity, EntityUid, PolicySet, Request, RestrictedExpression, Schema
    , SchemaFragment,
};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

// Define a simple test entity that implements HodeiEntity
#[derive(Debug, Clone)]
struct TestUser {
    id: String,
    name: String,
    email: String,
}

impl TestUser {
    fn new(id: String, name: String, email: String) -> Self {
        Self { id, name, email }
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
        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

#[test]
fn test_complete_schema_build() {
    // Test the complete schema
    let schema_str = r#"
    entity Principal { };
    
    entity User in Principal {
        name: String,
        email: String
    };
    
    action access appliesTo {
        principal: User,
        resource: User
    };
    "#;

    let result = SchemaFragment::from_cedarschema_str(schema_str);
    assert!(
        result.is_ok(),
        "Failed to create schema fragment: {:?}",
        result.err()
    );

    let (fragment, warnings) = result.unwrap();
    for warning in warnings {
        println!("Warning: {}", warning);
    }

    // Try to build a complete schema
    let schema_result = Schema::from_schema_fragments([fragment]);
    assert!(
        schema_result.is_ok(),
        "Failed to build complete schema: {:?}",
        schema_result.err()
    );

    let schema = schema_result.unwrap();

    // Try to create a validator
    let _validator = cedar_policy::Validator::new(schema.clone());

    // Test creating an entity with RestrictedExpression attributes
    let user = TestUser::new(
        "test_user".to_string(),
        "Test User".to_string(),
        "test@example.com".to_string(),
    );
    let parents: HashSet<_> = user.parents().into_iter().collect();
    let entity = Entity::new(user.euid(), user.attributes(), parents);
    assert!(
        entity.is_ok(),
        "Failed to create entity: {:?}",
        entity.err()
    );
}

#[test]
fn test_policy_evaluation_with_restricted_expressions() -> Result<(), Box<dyn std::error::Error>> {
    let schema_str = r#"
    entity Principal { };
    
    entity User in Principal {
        name: String,
        email: String
    };
    
    action access appliesTo {
        principal: User,
        resource: User
    };
    "#;

    let (schema_fragment, _) = SchemaFragment::from_cedarschema_str(schema_str)?;
    let schema = Schema::from_schema_fragments([schema_fragment])?;

    // Create a simple policy
    let policy_str = r#"permit(
        principal == User::"alice",
        action == Action::"access",
        resource == User::"bob"
    );"#;

    let policy = policy_str.parse()?;

    // Create entities
    let alice_attrs: HashMap<String, RestrictedExpression> = [
        (
            "name".to_string(),
            RestrictedExpression::new_string("Alice".to_string()),
        ),
        (
            "email".to_string(),
            RestrictedExpression::new_string("alice@example.com".to_string()),
        ),
    ]
    .into_iter()
    .collect();

    let alice_entity = Entity::new(
        EntityUid::from_str(r#"User::"alice""#)?,
        alice_attrs,
        HashSet::new(),
    )?;

    let bob_attrs: HashMap<String, RestrictedExpression> = [
        (
            "name".to_string(),
            RestrictedExpression::new_string("Bob".to_string()),
        ),
        (
            "email".to_string(),
            RestrictedExpression::new_string("bob@example.com".to_string()),
        ),
    ]
    .into_iter()
    .collect();

    let bob_entity = Entity::new(
        EntityUid::from_str(r#"User::"bob""#)?,
        bob_attrs,
        HashSet::new(),
    )?;

    let entities = Entities::from_entities(vec![alice_entity, bob_entity], None).expect("entities");
    let policies = PolicySet::from_policies([policy])?;

    let request = Request::new(
        EntityUid::from_str(r#"User::"alice""#)?,
        EntityUid::from_str(r#"Action::"access""#)?,
        EntityUid::from_str(r#"User::"bob""#)?,
        Context::empty(),
        Some(&schema),
    )?;

    let authorizer = cedar_policy::Authorizer::new();
    let response = authorizer.is_authorized(&request, &policies, &entities);

    assert_eq!(response.decision(), cedar_policy::Decision::Allow);
    Ok(())
}
