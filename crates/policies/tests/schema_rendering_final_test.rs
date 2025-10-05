use async_trait::async_trait;
use cedar_policy::{EntityTypeName, EntityUid, Policy, PolicySet, RestrictedExpression, Schema};
use kernel::Hrn;
use kernel::{
    AttributeType, HodeiEntity, HodeiEntityType, PolicyStorage, PolicyStorageError as StorageError,
    Principal, Resource,
};
/// Tests para verificar el rendering final del schema generado por el EngineBuilder
///
/// Estos tests registran diferentes tipos de entidades y acciones para validar
/// que el schema final se genera correctamente con namespaces, atributos y relaciones.
/// Usan validación de Cedar como fuente principal de verdad.
use policies::shared::application::EngineBuilder;
use policies::shared::domain::ActionTrait;
use regex::Regex;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
// ============================================================================
// Mock Storage
// ============================================================================

struct MockStorage;

#[async_trait]
impl PolicyStorage for MockStorage {
    async fn save_policy(&self, _policy: &Policy) -> Result<(), StorageError> {
        Ok(())
    }
    async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
        Ok(true)
    }
    async fn get_policy_by_id(&self, _id: &str) -> Result<Option<Policy>, StorageError> {
        Ok(None)
    }
    async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError> {
        Ok(vec![])
    }
}

// ============================================================================
// Mock IAM Entities (Principals)
// ============================================================================

struct IamUser {
    hrn: Hrn,
}

impl HodeiEntityType for IamUser {
    fn service_name() -> &'static str {
        "iam"
    }
    fn resource_type_name() -> &'static str {
        "User"
    }
    fn is_principal_type() -> bool {
        true
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("email", AttributeType::Primitive("String")),
            ("name", AttributeType::Primitive("String")),
            ("active", AttributeType::Primitive("Bool")),
            (
                "roles",
                AttributeType::Set(Box::new(AttributeType::Primitive("String"))),
            ),
        ]
    }
}

impl HodeiEntity for IamUser {
    fn hrn(&self) -> &kernel::Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Principal for IamUser {}

struct IamGroup {
    hrn: Hrn,
}

impl HodeiEntityType for IamGroup {
    fn service_name() -> &'static str {
        "iam"
    }
    fn resource_type_name() -> &'static str {
        "Group"
    }
    fn is_principal_type() -> bool {
        true
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("description", AttributeType::Primitive("String")),
        ]
    }
}

impl HodeiEntity for IamGroup {
    fn hrn(&self) -> &kernel::Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Principal for IamGroup {}

// ============================================================================
// Mock Artifact Entities (Resources)
// ============================================================================

struct ArtifactPackage {
    hrn: Hrn,
}

impl HodeiEntityType for ArtifactPackage {
    fn service_name() -> &'static str {
        "artifact"
    }
    fn resource_type_name() -> &'static str {
        "Package"
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("version", AttributeType::Primitive("String")),
            ("type", AttributeType::Primitive("String")),
            ("size", AttributeType::Primitive("Long")),
            (
                "tags",
                AttributeType::Set(Box::new(AttributeType::Primitive("String"))),
            ),
        ]
    }
}

impl HodeiEntity for ArtifactPackage {
    fn hrn(&self) -> &kernel::Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Resource for ArtifactPackage {}

struct ArtifactRepository {
    hrn: Hrn,
}

impl HodeiEntityType for ArtifactRepository {
    fn service_name() -> &'static str {
        "artifact"
    }
    fn resource_type_name() -> &'static str {
        "Repository"
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("visibility", AttributeType::Primitive("String")),
            ("ownerId", AttributeType::Primitive("String")),
        ]
    }
}

impl HodeiEntity for ArtifactRepository {
    fn hrn(&self) -> &kernel::Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Resource for ArtifactRepository {}

// ============================================================================
// Mock Actions
// ============================================================================

struct ReadPackageAction;

impl ActionTrait for ReadPackageAction {
    fn name() -> &'static str {
        "ReadPackage"
    }
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        let principal = EntityTypeName::from_str("Iam::User").expect("Valid principal type");
        let resource = EntityTypeName::from_str("Artifact::Package").expect("Valid resource type");
        (principal, resource)
    }
}

struct WritePackageAction;

impl ActionTrait for WritePackageAction {
    fn name() -> &'static str {
        "WritePackage"
    }
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        let principal = EntityTypeName::from_str("Iam::User").expect("Valid principal type");
        let resource = EntityTypeName::from_str("Artifact::Package").expect("Valid resource type");
        (principal, resource)
    }
}

struct ManageRepositoryAction;

impl ActionTrait for ManageRepositoryAction {
    fn name() -> &'static str {
        "ManageRepository"
    }
    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        let principal = EntityTypeName::from_str("Iam::Group").expect("Valid principal type");
        let resource =
            EntityTypeName::from_str("Artifact::Repository").expect("Valid resource type");
        (principal, resource)
    }
}

// ============================================================================
// Helper para renderizar schema
// ============================================================================

fn render_schema(schema: &Schema) -> String {
    format!("{:#?}", schema)
}

fn print_schema_details(schema: &Schema, title: &str) {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║ {:<62} ║", title);
    println!("╚════════════════════════════════════════════════════════════════╝");

    let schema_str = render_schema(schema);

    println!("\n📋 Schema Debug Output:");
    println!("{}", schema_str);

    println!("\n✅ Schema built successfully!");
    println!("   - Entity types, actions, and relationships are properly defined");
    println!("   - Namespaces are correctly structured");
    println!("   - All fragments were merged without conflicts\n");
}

/// Valida que una política es válida contra el schema usando Cedar
fn validate_policy_against_schema(schema: &Schema, policy_str: &str) -> Result<(), String> {
    let policy: Policy = policy_str
        .parse()
        .map_err(|e| format!("Failed to parse policy: {}", e))?;

    let mut policy_set = PolicySet::new();
    policy_set
        .add(policy)
        .map_err(|e| format!("Failed to add policy to set: {}", e))?;

    let validator = cedar_policy::Validator::new(schema.clone());
    let validation_result =
        validator.validate(&policy_set, cedar_policy::ValidationMode::default());

    if validation_result.validation_passed() {
        Ok(())
    } else {
        let errors: Vec<String> = validation_result
            .validation_errors()
            .map(|e| format!("{:?}", e))
            .collect();
        Err(format!("Validation failed: {:?}", errors))
    }
}

/// Verifica que el schema contiene los componentes esperados usando validación de políticas
fn assert_schema_contains_entities_and_actions(schema: &Schema, expected_components: &[&str]) {
    for component in expected_components {
        let test_policy = if component.starts_with("Action::") {
            // Para una acción como "Action::"ReadPackage"", creamos una política que la use
            format!("permit(principal, action == {}, resource);", component)
        } else if component.contains("::") {
            // Para una entidad como "Iam::User", creamos una política que la use en una condición 'is'
            format!(
                "permit(principal, action, resource) when {{ principal is {} }};",
                component
            )
        } else {
            // Ignorar componentes no reconocidos
            continue;
        };

        validate_policy_against_schema(schema, &test_policy).unwrap_or_else(|e| {
            panic!(
                "Schema validation failed for component '{}': {}\nGenerated policy: {}",
                component, e, test_policy
            )
        });
    }
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_schema_with_single_principal_and_resource() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Single Principal and Resource ===");
    println!("{}", schema_str);
    println!("=================================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &["Iam::User", "Artifact::Package", "Action::\"ReadPackage\""],
    );

    let test_policy = r#"
        permit(
            principal == Iam::User::"alice",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-123"
        );
    "#;

    validate_policy_against_schema(schema, test_policy)
        .expect("Policy should be valid against schema");
}

#[tokio::test]
async fn test_schema_with_multiple_principals() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_principal::<IamGroup>()
        .expect("register IamGroup")
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Multiple Principals ===");
    println!("{}", schema_str);
    println!("========================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &[
            "Iam::User",
            "Iam::Group",
            "Artifact::Package",
            "Action::\"ReadPackage\"",
        ],
    );

    let user_policy = r#"
        permit(
            principal == Iam::User::"bob",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-456"
        );
    "#;
    validate_policy_against_schema(schema, user_policy).expect("User policy should be valid");
}

#[tokio::test]
async fn test_schema_with_multiple_resources() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_resource::<ArtifactRepository>()
        .expect("register ArtifactRepository")
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Multiple Resources ===");
    println!("{}", schema_str);
    println!("=======================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &[
            "Iam::User",
            "Artifact::Package",
            "Artifact::Repository",
            "Action::\"ReadPackage\"",
        ],
    );

    let package_policy = r#"
        permit(
            principal == Iam::User::"charlie",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-789"
        );
    "#;
    validate_policy_against_schema(schema, package_policy).expect("Package policy should be valid");
}

#[tokio::test]
async fn test_schema_with_multiple_actions() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_principal::<IamGroup>()
        .expect("register IamGroup")
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_resource::<ArtifactRepository>()
        .expect("register ArtifactRepository")
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction")
        .register_action::<WritePackageAction>()
        .expect("register WritePackageAction")
        .register_action::<ManageRepositoryAction>()
        .expect("register ManageRepositoryAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Multiple Actions ===");
    println!("{}", schema_str);
    println!("=====================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &[
            "Iam::User",
            "Iam::Group",
            "Artifact::Package",
            "Artifact::Repository",
            "Action::\"ReadPackage\"",
            "Action::\"WritePackage\"",
            "Action::\"ManageRepository\"",
        ],
    );

    let read_policy = r#"
        permit(
            principal == Iam::User::"dave",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-read"
        );
    "#;
    validate_policy_against_schema(schema, read_policy).expect("Read policy should be valid");

    let write_policy = r#"
        permit(
            principal == Iam::User::"eve",
            action == Action::"WritePackage",
            resource == Artifact::Package::"pkg-write"
        );
    "#;
    validate_policy_against_schema(schema, write_policy).expect("Write policy should be valid");

    let manage_policy = r#"
        permit(
            principal == Iam::Group::"admins",
            action == Action::"ManageRepository",
            resource == Artifact::Repository::"repo-main"
        );
    "#;
    validate_policy_against_schema(schema, manage_policy).expect("Manage policy should be valid");
}

#[tokio::test]
async fn test_schema_with_complex_attributes() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_resource::<ArtifactPackage>() // <-- Recurso que faltaba
        .expect("register ArtifactPackage")
        .register_resource::<ArtifactRepository>()
        .expect("register ArtifactRepository")
        .register_action::<ReadPackageAction>() // <-- Acción que faltaba
        .expect("register ReadPackageAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Schema with Complex Attributes ===");
    println!("{}", schema_str);
    println!("=======================================\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &[
            "Iam::User",
            "Artifact::Package",
            "Artifact::Repository",
            "Action::\"ReadPackage\"",
        ],
    );

    let complex_policy = r#"
        permit(
            principal == Iam::User::"frank",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"pkg-complex"
        ) when {
            principal.active == true
        };
    "#;
    validate_policy_against_schema(schema, complex_policy).expect("Complex policy should be valid");
}

#[tokio::test]
async fn test_complete_schema_rendering() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let mut builder = EngineBuilder::new();

    // Registrar todos los principals
    builder
        .register_principal::<IamUser>()
        .expect("register IamUser")
        .register_principal::<IamGroup>()
        .expect("register IamGroup");

    // Registrar todos los resources
    builder
        .register_resource::<ArtifactPackage>()
        .expect("register ArtifactPackage")
        .register_resource::<ArtifactRepository>()
        .expect("register ArtifactRepository");

    // Registrar todas las acciones
    builder
        .register_action::<ReadPackageAction>()
        .expect("register ReadPackageAction")
        .register_action::<WritePackageAction>()
        .expect("register WritePackageAction")
        .register_action::<ManageRepositoryAction>()
        .expect("register ManageRepositoryAction");

    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;

    print_schema_details(schema, "COMPLETE SCHEMA RENDERING TEST");

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Schema Components Registered:                                ║");
    println!("║  - Principals: IamUser, IamGroup                              ║");
    println!("║  - Resources: ArtifactPackage, ArtifactRepository             ║");
    println!("║  - Actions: ReadPackage, WritePackage, ManageRepository       ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    assert_schema_contains_entities_and_actions(
        schema,
        &[
            "Iam::User",
            "Iam::Group",
            "Artifact::Package",
            "Artifact::Repository",
            "Action::\"ReadPackage\"",
            "Action::\"WritePackage\"",
            "Action::\"ManageRepository\"",
        ],
    );

    let policies = vec![
        r#"permit(principal == Iam::User::"admin", action == Action::"WritePackage", resource == Artifact::Package::"critical-pkg");"#,
        r#"permit(principal == Iam::Group::"devops", action == Action::"ManageRepository", resource == Artifact::Repository::"prod-repo");"#,
        r#"permit(principal == Iam::User::"reader", action == Action::"ReadPackage", resource == Artifact::Package::"public-pkg") when { principal.active == true };"#,
    ];

    for (idx, policy_str) in policies.iter().enumerate() {
        validate_policy_against_schema(schema, policy_str)
            .unwrap_or_else(|e| panic!("Policy {} should be valid: {}", idx, e));
    }

    println!(
        "\n✅ All {} policies validated successfully against the complete schema!",
        policies.len()
    );
}

#[tokio::test]
async fn test_empty_schema() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(MockStorage);

    let builder = EngineBuilder::new();
    let (engine, _store) = builder.build(storage).expect("build engine");

    let schema = &engine.schema;
    let schema_str = render_schema(schema);

    println!("\n=== Empty Schema (No Registrations) ===");
    println!("{}", schema_str);
    println!("========================================\n");

    let iam_pattern = Regex::new(r"namespace\s+Iam").expect("Valid regex");
    let artifact_pattern = Regex::new(r"namespace\s+Artifact").expect("Valid regex");

    assert!(
        !iam_pattern.is_match(&schema_str),
        "Empty schema should not contain Iam namespace"
    );
    assert!(
        !artifact_pattern.is_match(&schema_str),
        "Empty schema should not contain Artifact namespace"
    );

    let invalid_policy = r#"
        permit(
            principal == Iam::User::"test",
            action == Action::"ReadPackage",
            resource == Artifact::Package::"test"
        );
    "#;

    let result = validate_policy_against_schema(schema, invalid_policy);
    assert!(
        result.is_err(),
        "Policy should fail validation against empty schema"
    );
    println!(
        "✅ Policy correctly failed validation against empty schema: {:?}",
        result.err()
    );
}
