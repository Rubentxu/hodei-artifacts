//! Traits and types to describe domain entities in an agnostic way
//!
//! This module defines the fundamental abstractions so that domain
//! entities can integrate with the policy system without coupling to
//! any specific implementation (such as Cedar).
//!
//! # Design Principles
//!
//! - **Agnostic**: No dependencies on external policy engines
//! - **Type safe**: Uses Value Objects instead of primitive strings
//! - **Metadata and Runtime**: Separates type information (metadata) from instances
//! - **Extensible**: Allows bounded contexts to define their own entities
//!
//! # Examples
//!
//! ```ignore
//! use kernel::domain::{HodeiEntityType, HodeiEntity, AttributeValue};
//! use kernel::domain::{ServiceName, ResourceTypeName, AttributeName};
//!
//! struct User {
//!     hrn: Hrn,
//!     email: String,
//! }
//!
//! impl HodeiEntityType for User {
//!     fn service_name() -> ServiceName {
//!         ServiceName::new("iam").unwrap()
//!     }
//!
//!     fn resource_type_name() -> ResourceTypeName {
//!         ResourceTypeName::new("User").unwrap()
//!     }
//! }
//!
//! impl HodeiEntity for User {
//!     fn hrn(&self) -> &Hrn {
//!         &self.hrn
//!     }
//!
//!     fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
//!         let mut attrs = HashMap::new();
//!         attrs.insert(
//!             AttributeName::new("email").unwrap(),
//!             AttributeValue::string(&self.email)
//!         );
//!         attrs
//!     }
//! }
//! ```

use crate::domain::{AttributeName, AttributeValue, Hrn, ResourceTypeName, ServiceName};
use std::collections::HashMap;

// ============================================================================
// AttributeType - Attribute type metadata
// ============================================================================

/// Describes the type of an attribute for schema metadata
///
/// This enum is used to declare which attribute types an entity supports,
/// enabling validation and schema generation in an agnostic way.
///
/// # Examples
///
/// ```
/// use kernel::domain::AttributeType;
///
/// // Primitives
/// let string_type = AttributeType::String;
/// let long_type = AttributeType::Long;
/// let bool_type = AttributeType::Bool;
///
/// // Collections
/// let string_set = AttributeType::Set(Box::new(AttributeType::String));
///
/// // Entity references
/// let user_ref = AttributeType::EntityRef("User");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeType {
    /// Boolean type
    Bool,
    /// 64-bit integer
    Long,
    /// Text string
    String,
    /// Set of elements of the specified type
    Set(Box<AttributeType>),
    /// Record (Record/Map) with typed fields
    /// The HashMap holds the field name and its type
    Record(HashMap<String, AttributeType>),
    /// Reference to another entity by its type
    /// The &'static str must be the entity type name (e.g. "User", "Group")
    EntityRef(&'static str),
}

impl AttributeType {
    /// Creates an AttributeType::Bool
    pub const fn bool() -> Self {
        Self::Bool
    }

    /// Creates an AttributeType::Long
    pub const fn long() -> Self {
        Self::Long
    }

    /// Creates an AttributeType::String
    pub const fn string() -> Self {
        Self::String
    }

    /// Creates an AttributeType::Set
    pub fn set(inner: AttributeType) -> Self {
        Self::Set(Box::new(inner))
    }

    /// Creates an AttributeType::Record
    pub fn record(fields: HashMap<String, AttributeType>) -> Self {
        Self::Record(fields)
    }

    /// Creates an AttributeType::EntityRef
    pub const fn entity_ref(entity_type: &'static str) -> Self {
        Self::EntityRef(entity_type)
    }

    /// Returns a string representation of the type (useful for debugging and schemas)
    pub fn type_name(&self) -> String {
        match self {
            Self::Bool => "Bool".to_string(),
            Self::Long => "Long".to_string(),
            Self::String => "String".to_string(),
            Self::Set(inner) => format!("Set<{}>", inner.type_name()),
            Self::Record(_) => "Record".to_string(),
            Self::EntityRef(ty) => format!("EntityRef<{}>", ty),
        }
    }

    /// Returns the type declaration for the Cedar schema
    pub fn to_cedar_decl(&self) -> String {
        match self {
            Self::Bool => "Bool".to_string(),
            Self::Long => "Long".to_string(),
            Self::String => "String".to_string(),
            Self::Set(inner) => format!("Set<{}>", inner.to_cedar_decl()),
            Self::Record(_) => "Record".to_string(),
            Self::EntityRef(ty) => format!("EntityRef<{}>", ty),
        }
    }
}

// ============================================================================
// HodeiEntityType - Type-level metadata
// ============================================================================

/// Type-level metadata for domain entities
///
/// This trait must be implemented on the entity's type (struct), not on
/// instances. It provides static information about the entity that can be
/// used to generate schemas, validate policies, etc.
///
/// # Implementation
///
/// All methods are associated (they do not require `self`), since they describe
/// the type itself, not particular instances.
///
/// # Example
///
/// ```ignore
/// impl HodeiEntityType for User {
///     fn service_name() -> ServiceName {
///         ServiceName::new("iam").unwrap()
///     }
///
///     fn resource_type_name() -> ResourceTypeName {
///         ResourceTypeName::new("User").unwrap()
///     }
///
///     fn is_principal_type() -> bool {
///         true // Users can be principals
///     }
///
///     fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
///         vec![
///             (AttributeName::new("email").unwrap(), AttributeType::String),
///             (AttributeName::new("age").unwrap(), AttributeType::Long),
///         ]
///     }
/// }
/// ```
pub trait HodeiEntityType {
    /// Name of the service (logical namespace) this entity belongs to
    ///
    /// For example: "iam", "organizations", "supply-chain"
    fn service_name() -> ServiceName;

    /// Name of the resource type
    ///
    /// For example: "User", "Group", "Account", "ServiceControlPolicy"
    fn resource_type_name() -> ResourceTypeName;

    /// Full name of the entity type (Service::Type)
    ///
    /// This method has a default implementation that combines
    /// service_name and resource_type_name.
    ///
    /// Returns a string such as "Iam::User" or "Organizations::Account"
    fn entity_type_name() -> String {
        let service = Self::service_name();
        let resource = Self::resource_type_name();
        let namespace = crate::domain::hrn::Hrn::to_pascal_case(service.as_str());
        format!("{}::{}", namespace, resource.as_str())
    }

    /// Indicates whether this type can act as a Principal in policies
    ///
    /// A principal is the entity that performs an action (e.g. User, ServiceAccount)
    fn is_principal_type() -> bool {
        false
    }

    /// Indicates whether this type can act as a Resource in policies
    ///
    /// A resource is the entity on which the action is performed
    fn is_resource_type() -> bool {
        true
    }

    /// Attribute schema declared by this entity
    ///
    /// Returns a list of (attribute_name, attribute_type) pairs that
    /// describes the attributes instances of this type may have.
    ///
    /// This is useful for validation and schema generation.
    fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
        Vec::new()
    }

    /// Entity types that can be parents (hierarchy)
    ///
    /// For example, a User can have a Group as parent.
    /// Returns a list of entity type names.
    fn parent_types() -> Vec<String> {
        Vec::new()
    }
}

// ============================================================================
// HodeiEntity - Runtime instance of an entity
// ============================================================================

/// Represents a concrete (runtime) instance of a domain entity
///
/// This trait is implemented on entity instances and provides
/// access to their data at runtime.
///
/// # Example
///
/// ```ignore
/// impl HodeiEntity for User {
///     fn hrn(&self) -> &Hrn {
///         &self.hrn
///     }
///
///     fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
///         let mut attrs = HashMap::new();
///         attrs.insert(
///             AttributeName::new("email").unwrap(),
///             AttributeValue::string(&self.email)
///         );
///         attrs.insert(
///             AttributeName::new("active").unwrap(),
///             AttributeValue::bool(self.is_active)
///         );
///         attrs
///     }
///
///     fn parent_hrns(&self) -> Vec<Hrn> {
///         // Return the HRNs of the groups it belongs to
///         self.group_hrns.clone()
///     }
/// }
/// ```
pub trait HodeiEntity: std::fmt::Debug + Send + Sync {
    /// Returns the canonical HRN (Hodei Resource Name) of this entity
    ///
    /// The HRN is the unique, global identifier of the entity.
    fn hrn(&self) -> &Hrn;

    /// Returns this entity's attributes as a key-value map
    ///
    /// Attributes are additional properties of the entity that may
    /// be used in policy evaluation.
    fn attributes(&self) -> HashMap<AttributeName, AttributeValue>;

    /// Returns the HRNs of the parent entities (hierarchy/membership)
    ///
    /// For example, a User may return the HRNs of the Groups it belongs to.
    /// By default returns an empty vector (no parents).
    fn parent_hrns(&self) -> Vec<Hrn> {
        Vec::new()
    }

    /// Returns this entity's attributes in a Cedar-compatible format
    ///
    /// This is an optional extension of the trait that allows entities to
    /// provide their attributes in a format Cedar can understand directly.
    /// By default, it converts the standard attributes to a compatible format.
    fn cedar_attributes(&self) -> Option<Vec<(String, crate::domain::AttributeType)>> {
        // Convert the standard attributes to Cedar format
        let mut cedar_attrs = Vec::new();
        for (name, value) in self.attributes() {
            let cedar_type = match value {
                AttributeValue::String(_) => crate::domain::AttributeType::string(),
                AttributeValue::Long(_) => crate::domain::AttributeType::long(),
                AttributeValue::Bool(_) => crate::domain::AttributeType::bool(),
                AttributeValue::Set(set) => {
                    if let Some(first) = set.first() {
                        let element_type = match first {
                            AttributeValue::String(_) => crate::domain::AttributeType::string(),
                            AttributeValue::Long(_) => crate::domain::AttributeType::long(),
                            AttributeValue::Bool(_) => crate::domain::AttributeType::bool(),
                            AttributeValue::Set(_) => crate::domain::AttributeType::string(), // Nested, use String
                            AttributeValue::Record(_) => crate::domain::AttributeType::string(),
                            AttributeValue::EntityRef(_) => crate::domain::AttributeType::string(),
                        };
                        crate::domain::AttributeType::set(element_type)
                    } else {
                        crate::domain::AttributeType::set(crate::domain::AttributeType::string())
                    }
                }
                AttributeValue::Record(_) => crate::domain::AttributeType::string(), // Simplified
                AttributeValue::EntityRef(_) => crate::domain::AttributeType::string(), // Simplified
            };
            cedar_attrs.push((name.as_str().to_string(), cedar_type));
        }
        Some(cedar_attrs)
    }
}

// ============================================================================
// Marker Traits for policy roles
// ============================================================================

/// Marker trait for entities that can act as Principal
///
/// A Principal is the entity that performs an action (e.g. User, ServiceAccount).
/// This trait requires the entity to implement both `HodeiEntity` (runtime)
/// and `HodeiEntityType` (metadata).
pub trait Principal: HodeiEntity + HodeiEntityType {}

/// Marker trait for entities that can act as Resource
///
/// A Resource is the entity on which an action is performed.
/// This trait requires the entity to implement both `HodeiEntity` (runtime)
/// and `HodeiEntityType` (metadata).
pub trait Resource: HodeiEntity + HodeiEntityType {}

// ============================================================================
// ActionTrait - Defines domain actions
// ============================================================================

/// Defines an action that can be performed in the system
///
/// Actions are operations that a Principal can perform on a Resource.
///
/// # Example
///
/// ```ignore
/// pub struct CreateUserAction;
///
/// impl ActionTrait for CreateUserAction {
///     fn name() -> &'static str {
///         "CreateUser"
///     }
///
///     fn service_name() -> ServiceName {
///         ServiceName::new("iam").unwrap()
///     }
///
///     fn applies_to_principal() -> String {
///         "Iam::User".to_string()
///     }
///
///     fn applies_to_resource() -> String {
///         "Iam::User".to_string()
///     }
/// }
/// ```
pub trait ActionTrait {
    /// Unique identifying name of the action
    ///
    /// For example: "CreateUser", "DeleteGroup", "AttachPolicy"
    fn name() -> &'static str;

    /// Name of the service this action belongs to
    fn service_name() -> ServiceName;

    /// Full name of the action (`Action::"Nombre"`)
    ///
    /// The action namespace is **flat**: it is the form the registered Cedar
    /// schema declares its action types with and the one the evaluation engine
    /// builds when resolving a request. The three paths that used to produce
    /// divergent forms (`Iam::Action::"X"` here, `Action::"X"` in the engine)
    /// now produce the same one.
    fn action_name() -> String {
        format!("Action::\"{}\"", Self::name())
    }

    /// Principal type that can perform this action
    ///
    /// Returns the full type name (e.g. "Iam::User")
    fn applies_to_principal() -> String;

    /// Resource type on which this action can be performed
    ///
    /// Returns the full type name (e.g. "Iam::Group")
    fn applies_to_resource() -> String;
}

// ============================================================================
// PolicyStorage - Policy persistence abstraction
// ============================================================================

/// Abstraction for storing and retrieving policies
///
/// This trait defines the interface to persist policies as strings.
/// Policies are stored in their textual format (e.g. Cedar DSL, Rego, etc.)
/// without coupling the kernel to any specific format.
#[async_trait::async_trait]
pub trait PolicyStorage: Send + Sync {
    /// Saves a policy
    ///
    /// # Parameters
    /// - `id`: Unique identifier of the policy
    /// - `policy_text`: The policy text in its DSL format
    async fn save_policy(&self, id: &str, policy_text: &str) -> Result<(), PolicyStorageError>;

    /// Deletes a policy by its ID
    ///
    /// Returns `true` if the policy existed and was deleted, `false` if it did not exist.
    async fn delete_policy(&self, id: &str) -> Result<bool, PolicyStorageError>;

    /// Retrieves a policy by its ID
    ///
    /// Returns `None` if the policy does not exist.
    async fn get_policy_by_id(&self, id: &str) -> Result<Option<String>, PolicyStorageError>;

    /// Loads all stored policies
    ///
    /// Returns a list of (id, policy_text) tuples
    async fn load_all_policies(&self) -> Result<Vec<(String, String)>, PolicyStorageError>;
}

/// Persistence layer errors for policies
#[derive(thiserror::Error, Debug)]
pub enum PolicyStorageError {
    /// Generic storage provider error
    #[error("Underlying storage error: {0}")]
    ProviderError(#[from] Box<dyn std::error::Error + Send + Sync>),

    /// Error parsing a policy
    #[error("Policy parsing error: {0}")]
    ParsingError(String),

    /// Policy not found
    #[error("Policy not found: {0}")]
    PolicyNotFound(String),

    /// Validation error
    #[error("Validation error: {0}")]
    ValidationError(String),
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // AttributeType tests
    // ========================================================================

    #[test]
    fn attribute_type_primitives() {
        assert_eq!(AttributeType::bool().type_name(), "Bool");
        assert_eq!(AttributeType::long().type_name(), "Long");
        assert_eq!(AttributeType::string().type_name(), "String");
    }

    #[test]
    fn attribute_type_set() {
        let string_set = AttributeType::set(AttributeType::string());
        assert_eq!(string_set.type_name(), "Set<String>");
    }

    #[test]
    fn attribute_type_entity_ref() {
        let user_ref = AttributeType::entity_ref("User");
        assert_eq!(user_ref.type_name(), "EntityRef<User>");
    }

    #[test]
    fn attribute_type_nested_set() {
        let nested = AttributeType::set(AttributeType::set(AttributeType::long()));
        assert_eq!(nested.type_name(), "Set<Set<Long>>");
    }

    // ========================================================================
    // HodeiEntityType tests
    // ========================================================================

    struct TestUser;

    impl HodeiEntityType for TestUser {
        fn service_name() -> ServiceName {
            ServiceName::new("iam").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("User").unwrap()
        }

        fn is_principal_type() -> bool {
            true
        }

        fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
            vec![
                (
                    AttributeName::new("email").unwrap(),
                    AttributeType::string(),
                ),
                (AttributeName::new("age").unwrap(), AttributeType::long()),
            ]
        }
    }

    #[test]
    fn entity_type_basic_info() {
        assert_eq!(TestUser::service_name().as_str(), "iam");
        assert_eq!(TestUser::resource_type_name().as_str(), "User");
        assert!(TestUser::is_principal_type());
    }

    #[test]
    fn entity_type_full_name() {
        let full_name = TestUser::entity_type_name();
        assert_eq!(full_name, "Iam::User");
    }

    #[test]
    fn entity_type_attributes_schema() {
        let schema = TestUser::attributes_schema();
        assert_eq!(schema.len(), 2);
        assert_eq!(schema[0].0.as_str(), "email");
        assert_eq!(schema[1].0.as_str(), "age");
    }

    // ========================================================================
    // HodeiEntity tests
    // ========================================================================

    #[derive(Debug)]
    struct TestUserInstance {
        hrn: Hrn,
        email: String,
        age: i64,
    }

    impl TestUserInstance {
        fn new(partition: String, account: String, id: String, email: String, age: i64) -> Self {
            Self {
                hrn: Hrn::for_entity_type::<TestUser>(partition, account, id),
                email,
                age,
            }
        }
    }

    impl HodeiEntity for TestUserInstance {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }

        fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
            let mut attrs = HashMap::new();
            attrs.insert(
                AttributeName::new("email").unwrap(),
                AttributeValue::string(&self.email),
            );
            attrs.insert(
                AttributeName::new("age").unwrap(),
                AttributeValue::long(self.age),
            );
            attrs
        }
    }

    #[test]
    fn entity_instance_hrn() {
        let user = TestUserInstance::new(
            "aws".to_string(),
            "123456789012".to_string(),
            "alice".to_string(),
            "alice@example.com".to_string(),
            30,
        );

        let hrn = user.hrn();
        assert_eq!(hrn.service(), "iam");
        assert_eq!(hrn.resource_id(), "alice");
    }

    #[test]
    fn entity_instance_attributes() {
        let user = TestUserInstance::new(
            "aws".to_string(),
            "123456789012".to_string(),
            "alice".to_string(),
            "alice@example.com".to_string(),
            30,
        );

        let attrs = user.attributes();
        assert_eq!(attrs.len(), 2);

        let email = attrs.get(&AttributeName::new("email").unwrap()).unwrap();
        assert_eq!(email.as_string(), Some("alice@example.com"));

        let age = attrs.get(&AttributeName::new("age").unwrap()).unwrap();
        assert_eq!(age.as_long(), Some(30));
    }

    // ========================================================================
    // ActionTrait tests
    // ========================================================================

    struct CreateUserAction;

    impl ActionTrait for CreateUserAction {
        fn name() -> &'static str {
            "CreateUser"
        }

        fn service_name() -> ServiceName {
            ServiceName::new("iam").unwrap()
        }

        fn applies_to_principal() -> String {
            "Iam::User".to_string()
        }

        fn applies_to_resource() -> String {
            "Iam::User".to_string()
        }
    }

    #[test]
    fn action_trait_basic() {
        assert_eq!(CreateUserAction::name(), "CreateUser");
        assert_eq!(CreateUserAction::service_name().as_str(), "iam");
    }

    #[test]
    fn action_trait_full_name() {
        let action_name = CreateUserAction::action_name();
        assert_eq!(action_name, "Action::\"CreateUser\"");
    }

    #[test]
    fn action_trait_applies_to() {
        assert_eq!(CreateUserAction::applies_to_principal(), "Iam::User");
        assert_eq!(CreateUserAction::applies_to_resource(), "Iam::User");
    }

    // ========================================================================
    // PolicyStorage tests
    // ========================================================================

    struct InMemoryPolicyStorage {
        items: std::sync::Mutex<HashMap<String, String>>,
    }

    impl InMemoryPolicyStorage {
        fn new() -> Self {
            Self {
                items: std::sync::Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl PolicyStorage for InMemoryPolicyStorage {
        async fn save_policy(&self, id: &str, policy_text: &str) -> Result<(), PolicyStorageError> {
            let mut guard = self.items.lock().unwrap();
            guard.insert(id.to_string(), policy_text.to_string());
            Ok(())
        }

        async fn delete_policy(&self, id: &str) -> Result<bool, PolicyStorageError> {
            let mut guard = self.items.lock().unwrap();
            Ok(guard.remove(id).is_some())
        }

        async fn get_policy_by_id(&self, id: &str) -> Result<Option<String>, PolicyStorageError> {
            let guard = self.items.lock().unwrap();
            Ok(guard.get(id).cloned())
        }

        async fn load_all_policies(&self) -> Result<Vec<(String, String)>, PolicyStorageError> {
            let guard = self.items.lock().unwrap();
            Ok(guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        }
    }

    #[tokio::test]
    async fn policy_storage_save_and_retrieve() {
        let storage = InMemoryPolicyStorage::new();
        let policy_text = "permit(principal, action, resource);";

        storage.save_policy("policy1", policy_text).await.unwrap();

        let retrieved = storage.get_policy_by_id("policy1").await.unwrap();
        assert_eq!(retrieved, Some(policy_text.to_string()));
    }

    #[tokio::test]
    async fn policy_storage_delete() {
        let storage = InMemoryPolicyStorage::new();
        storage.save_policy("policy1", "test policy").await.unwrap();

        let deleted = storage.delete_policy("policy1").await.unwrap();
        assert!(deleted);

        let retrieved = storage.get_policy_by_id("policy1").await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn policy_storage_load_all() {
        let storage = InMemoryPolicyStorage::new();
        storage.save_policy("p1", "policy 1").await.unwrap();
        storage.save_policy("p2", "policy 2").await.unwrap();

        let all = storage.load_all_policies().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    // ========================================================================
    // Additional tests for AttributeType::Record
    // ========================================================================

    #[test]
    fn attribute_type_record_simple() {
        let mut fields = HashMap::new();
        fields.insert("name".to_string(), AttributeType::string());
        fields.insert("age".to_string(), AttributeType::long());

        let record = AttributeType::record(fields);
        assert_eq!(record.type_name(), "Record");
    }

    #[test]
    fn attribute_type_record_nested() {
        let mut inner_record = HashMap::new();
        inner_record.insert("street".to_string(), AttributeType::string());
        inner_record.insert("city".to_string(), AttributeType::string());

        let mut outer_record = HashMap::new();
        outer_record.insert("name".to_string(), AttributeType::string());
        outer_record.insert("address".to_string(), AttributeType::record(inner_record));

        let record = AttributeType::record(outer_record);
        assert_eq!(record.type_name(), "Record");
    }

    #[test]
    fn attribute_type_record_with_sets() {
        let mut fields = HashMap::new();
        fields.insert(
            "tags".to_string(),
            AttributeType::set(AttributeType::string()),
        );
        fields.insert(
            "scores".to_string(),
            AttributeType::set(AttributeType::long()),
        );

        let record = AttributeType::record(fields);
        assert_eq!(record.type_name(), "Record");
    }

    #[test]
    fn attribute_type_deeply_nested() {
        let level3 = AttributeType::set(AttributeType::long());
        let level2 = AttributeType::set(level3);
        let level1 = AttributeType::set(level2);

        assert_eq!(level1.type_name(), "Set<Set<Set<Long>>>");
    }

    // ========================================================================
    // Tests for parent_types in HodeiEntityType
    // ========================================================================

    struct TestUserWithParents;

    impl HodeiEntityType for TestUserWithParents {
        fn service_name() -> ServiceName {
            ServiceName::new("iam").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("User").unwrap()
        }

        fn is_principal_type() -> bool {
            true
        }

        fn parent_types() -> Vec<String> {
            vec!["Iam::Group".to_string(), "Iam::Role".to_string()]
        }
    }

    #[test]
    fn entity_type_parent_types() {
        let parents = TestUserWithParents::parent_types();
        assert_eq!(parents.len(), 2);
        assert!(parents.contains(&"Iam::Group".to_string()));
        assert!(parents.contains(&"Iam::Role".to_string()));
    }

    #[test]
    fn entity_type_parent_types_empty_by_default() {
        let parents = TestUser::parent_types();
        assert_eq!(parents.len(), 0);
    }

    // ========================================================================
    // Tests for parent_hrns in HodeiEntity
    // ========================================================================

    #[derive(Debug)]
    struct TestUserWithParentHrns {
        hrn: Hrn,
        email: String,
        group_hrns: Vec<Hrn>,
    }

    impl TestUserWithParentHrns {
        fn new(
            partition: String,
            account: String,
            id: String,
            email: String,
            groups: Vec<Hrn>,
        ) -> Self {
            Self {
                hrn: Hrn::for_entity_type::<TestUser>(partition, account, id),
                email,
                group_hrns: groups,
            }
        }
    }

    impl HodeiEntity for TestUserWithParentHrns {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }

        fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
            let mut attrs = HashMap::new();
            attrs.insert(
                AttributeName::new("email").unwrap(),
                AttributeValue::string(&self.email),
            );
            attrs
        }

        fn parent_hrns(&self) -> Vec<Hrn> {
            self.group_hrns.clone()
        }
    }

    #[test]
    fn entity_parent_hrns() {
        let group1 = Hrn::for_entity_type::<TestUser>(
            "aws".to_string(),
            "123456789012".to_string(),
            "admins".to_string(),
        );
        let group2 = Hrn::for_entity_type::<TestUser>(
            "aws".to_string(),
            "123456789012".to_string(),
            "developers".to_string(),
        );

        let user = TestUserWithParentHrns::new(
            "aws".to_string(),
            "123456789012".to_string(),
            "alice".to_string(),
            "alice@example.com".to_string(),
            vec![group1.clone(), group2.clone()],
        );

        let parents = user.parent_hrns();
        assert_eq!(parents.len(), 2);
        assert_eq!(parents[0], group1);
        assert_eq!(parents[1], group2);
    }

    #[test]
    fn entity_parent_hrns_empty_by_default() {
        let user = TestUserInstance::new(
            "aws".to_string(),
            "123456789012".to_string(),
            "alice".to_string(),
            "alice@example.com".to_string(),
            30,
        );

        let parents = user.parent_hrns();
        assert_eq!(parents.len(), 0);
    }

    // ========================================================================
    // Tests for Principal and Resource marker traits
    // ========================================================================

    #[derive(Debug)]
    struct TestPrincipal {
        hrn: Hrn,
    }

    impl HodeiEntityType for TestPrincipal {
        fn service_name() -> ServiceName {
            ServiceName::new("iam").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("Principal").unwrap()
        }

        fn is_principal_type() -> bool {
            true
        }
    }

    impl HodeiEntity for TestPrincipal {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }

        fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
            HashMap::new()
        }
    }

    impl Principal for TestPrincipal {}

    #[derive(Debug)]
    struct TestResource {
        hrn: Hrn,
    }

    impl HodeiEntityType for TestResource {
        fn service_name() -> ServiceName {
            ServiceName::new("iam").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("Resource").unwrap()
        }

        fn is_resource_type() -> bool {
            true
        }
    }

    impl HodeiEntity for TestResource {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }

        fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
            HashMap::new()
        }
    }

    impl Resource for TestResource {}

    #[test]
    fn principal_trait_implementation() {
        let principal = TestPrincipal {
            hrn: Hrn::for_entity_type::<TestPrincipal>(
                "aws".to_string(),
                "123456789012".to_string(),
                "test-principal".to_string(),
            ),
        };

        // Verify that it implements HodeiEntity
        assert_eq!(principal.hrn().resource_id(), "test-principal");
        assert!(TestPrincipal::is_principal_type());
    }

    #[test]
    fn resource_trait_implementation() {
        let resource = TestResource {
            hrn: Hrn::for_entity_type::<TestResource>(
                "aws".to_string(),
                "123456789012".to_string(),
                "test-resource".to_string(),
            ),
        };

        // Verify that it implements HodeiEntity
        assert_eq!(resource.hrn().resource_id(), "test-resource");
        assert!(TestResource::is_resource_type());
    }

    // ========================================================================
    // Tests for is_resource_type with default value
    // ========================================================================

    #[test]
    fn entity_type_is_resource_by_default() {
        // TestUser does not override is_resource_type, should be true by default
        assert!(TestUser::is_resource_type());
    }

    #[test]
    fn entity_type_is_not_principal_by_default() {
        struct DefaultEntity;

        impl HodeiEntityType for DefaultEntity {
            fn service_name() -> ServiceName {
                ServiceName::new("test").unwrap()
            }

            fn resource_type_name() -> ResourceTypeName {
                ResourceTypeName::new("Default").unwrap()
            }
        }

        // is_principal_type() should be false by default
        assert!(!DefaultEntity::is_principal_type());
        // is_resource_type() should be true by default
        assert!(DefaultEntity::is_resource_type());
    }

    // ========================================================================
    // Additional tests for PolicyStorage with errors
    // ========================================================================

    struct FailingPolicyStorage;

    #[async_trait::async_trait]
    impl PolicyStorage for FailingPolicyStorage {
        async fn save_policy(
            &self,
            _id: &str,
            _policy_text: &str,
        ) -> Result<(), PolicyStorageError> {
            Err(PolicyStorageError::ValidationError(
                "Invalid policy".to_string(),
            ))
        }

        async fn delete_policy(&self, _id: &str) -> Result<bool, PolicyStorageError> {
            Err(PolicyStorageError::PolicyNotFound("Not found".to_string()))
        }

        async fn get_policy_by_id(&self, _id: &str) -> Result<Option<String>, PolicyStorageError> {
            Err(PolicyStorageError::ParsingError("Parse failed".to_string()))
        }

        async fn load_all_policies(&self) -> Result<Vec<(String, String)>, PolicyStorageError> {
            Err(PolicyStorageError::ProviderError(Box::new(
                std::io::Error::other("Provider error"),
            )))
        }
    }

    #[tokio::test]
    async fn policy_storage_save_validation_error() {
        let storage = FailingPolicyStorage;
        let result = storage.save_policy("p1", "invalid").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            PolicyStorageError::ValidationError(msg) => {
                assert_eq!(msg, "Invalid policy");
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn policy_storage_delete_not_found_error() {
        let storage = FailingPolicyStorage;
        let result = storage.delete_policy("p1").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            PolicyStorageError::PolicyNotFound(msg) => {
                assert_eq!(msg, "Not found");
            }
            _ => panic!("Expected PolicyNotFound"),
        }
    }

    #[tokio::test]
    async fn policy_storage_get_parsing_error() {
        let storage = FailingPolicyStorage;
        let result = storage.get_policy_by_id("p1").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            PolicyStorageError::ParsingError(msg) => {
                assert_eq!(msg, "Parse failed");
            }
            _ => panic!("Expected ParsingError"),
        }
    }

    #[tokio::test]
    async fn policy_storage_load_all_provider_error() {
        let storage = FailingPolicyStorage;
        let result = storage.load_all_policies().await;

        assert!(result.is_err());
        match result.unwrap_err() {
            PolicyStorageError::ProviderError(_) => {
                // Success - we got the expected error type
            }
            _ => panic!("Expected ProviderError"),
        }
    }

    #[tokio::test]
    async fn policy_storage_get_nonexistent_returns_none() {
        let storage = InMemoryPolicyStorage::new();
        let result = storage.get_policy_by_id("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn policy_storage_delete_nonexistent_returns_false() {
        let storage = InMemoryPolicyStorage::new();
        let deleted = storage.delete_policy("nonexistent").await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn policy_storage_update_existing() {
        let storage = InMemoryPolicyStorage::new();

        // Save initial policy
        storage.save_policy("p1", "version 1").await.unwrap();
        let v1 = storage.get_policy_by_id("p1").await.unwrap();
        assert_eq!(v1, Some("version 1".to_string()));

        // Update with new version
        storage.save_policy("p1", "version 2").await.unwrap();
        let v2 = storage.get_policy_by_id("p1").await.unwrap();
        assert_eq!(v2, Some("version 2".to_string()));
    }

    // ========================================================================
    // Tests for default attributes_schema
    // ========================================================================

    #[test]
    fn entity_type_attributes_schema_empty_by_default() {
        struct MinimalEntity;

        impl HodeiEntityType for MinimalEntity {
            fn service_name() -> ServiceName {
                ServiceName::new("test").unwrap()
            }

            fn resource_type_name() -> ResourceTypeName {
                ResourceTypeName::new("Minimal").unwrap()
            }
        }

        let schema = MinimalEntity::attributes_schema();
        assert_eq!(schema.len(), 0);
    }
}
