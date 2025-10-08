//! Traits y tipos para describir entidades del dominio de forma agnóstica
//!
//! Este módulo define las abstracciones fundamentales para que las entidades
//! de dominio puedan integrarse con el sistema de políticas sin acoplarse a
//! ninguna implementación específica (como Cedar).
//!
//! # Principios de Diseño
//!
//! - **Agnóstico**: Sin dependencias de motores de políticas externos
//! - **Tipo seguro**: Usa Value Objects en lugar de strings primitivos
//! - **Metadata y Runtime**: Separa información de tipo (metadata) de instancias
//! - **Extensible**: Permite que bounded contexts definan sus propias entidades
//!
//! # Ejemplos
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
// AttributeType - Metadata de tipos de atributos
// ============================================================================

/// Describe el tipo de un atributo para metadatos de esquema
///
/// Este enum se usa para declarar qué tipos de atributos soporta una entidad,
/// permitiendo validación y generación de esquemas de forma agnóstica.
///
/// # Ejemplos
///
/// ```
/// use kernel::domain::AttributeType;
///
/// // Primitivos
/// let string_type = AttributeType::String;
/// let long_type = AttributeType::Long;
/// let bool_type = AttributeType::Bool;
///
/// // Colecciones
/// let string_set = AttributeType::Set(Box::new(AttributeType::String));
///
/// // Referencias a entidades
/// let user_ref = AttributeType::EntityRef("User");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeType {
    /// Tipo booleano
    Bool,
    /// Entero de 64 bits
    Long,
    /// Cadena de texto
    String,
    /// Conjunto (Set) de elementos del tipo especificado
    Set(Box<AttributeType>),
    /// Registro (Record/Map) con campos tipados
    /// El HashMap contiene el nombre del campo y su tipo
    Record(HashMap<String, AttributeType>),
    /// Referencia a otra entidad por su tipo
    /// El &'static str debe ser el nombre del tipo de entidad (ej: "User", "Group")
    EntityRef(&'static str),
}

impl AttributeType {
    /// Crea un AttributeType::Bool
    pub const fn bool() -> Self {
        Self::Bool
    }

    /// Crea un AttributeType::Long
    pub const fn long() -> Self {
        Self::Long
    }

    /// Crea un AttributeType::String
    pub const fn string() -> Self {
        Self::String
    }

    /// Crea un AttributeType::Set
    pub fn set(inner: AttributeType) -> Self {
        Self::Set(Box::new(inner))
    }

    /// Crea un AttributeType::Record
    pub fn record(fields: HashMap<String, AttributeType>) -> Self {
        Self::Record(fields)
    }

    /// Crea un AttributeType::EntityRef
    pub const fn entity_ref(entity_type: &'static str) -> Self {
        Self::EntityRef(entity_type)
    }

    /// Retorna una representación en string del tipo (útil para debugging y schemas)
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

    /// Retorna la declaración de tipo para el schema de Cedar
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
// HodeiEntityType - Metadata a nivel de tipo
// ============================================================================

/// Metadata a nivel de tipo para entidades del dominio
///
/// Este trait debe implementarse en el tipo (struct) de la entidad, no en
/// instancias. Proporciona información estática sobre la entidad que puede
/// usarse para generar esquemas, validar políticas, etc.
///
/// # Implementación
///
/// Todos los métodos son asociados (no requieren `self`), ya que describen
/// el tipo en sí, no instancias particulares.
///
/// # Ejemplo
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
///         true // Los usuarios pueden ser principals
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
    /// Nombre del servicio (namespace lógico) al que pertenece esta entidad
    ///
    /// Por ejemplo: "iam", "organizations", "supply-chain"
    fn service_name() -> ServiceName;

    /// Nombre del tipo de recurso
    ///
    /// Por ejemplo: "User", "Group", "Account", "ServiceControlPolicy"
    fn resource_type_name() -> ResourceTypeName;

    /// Nombre completo del tipo de entidad (Servicio::Tipo)
    ///
    /// Este método tiene una implementación por defecto que combina
    /// el service_name y resource_type_name.
    ///
    /// Retorna un string como "Iam::User" o "Organizations::Account"
    fn entity_type_name() -> String {
        let service = Self::service_name();
        let resource = Self::resource_type_name();
        let namespace = crate::domain::hrn::Hrn::to_pascal_case(service.as_str());
        format!("{}::{}", namespace, resource.as_str())
    }

    /// Indica si este tipo puede actuar como Principal en políticas
    ///
    /// Un principal es la entidad que realiza una acción (ej: User, ServiceAccount)
    fn is_principal_type() -> bool {
        false
    }

    /// Indica si este tipo puede actuar como Resource en políticas
    ///
    /// Un resource es la entidad sobre la que se realiza la acción
    fn is_resource_type() -> bool {
        true
    }

    /// Esquema de atributos que declara esta entidad
    ///
    /// Retorna una lista de pares (nombre_atributo, tipo_atributo) que
    /// describe los atributos que las instancias de este tipo pueden tener.
    ///
    /// Esto es útil para validación y generación de esquemas.
    fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
        Vec::new()
    }

    /// Tipos de entidades que pueden ser parents (jerarquía)
    ///
    /// Por ejemplo, un User puede tener como parent un Group.
    /// Retorna una lista de nombres de tipos de entidad.
    fn parent_types() -> Vec<String> {
        Vec::new()
    }
}

// ============================================================================
// HodeiEntity - Instancia runtime de una entidad
// ============================================================================

/// Representa una instancia concreta (runtime) de una entidad del dominio
///
/// Este trait se implementa en instancias de entidades y proporciona
/// acceso a sus datos en runtime.
///
/// # Ejemplo
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
///         // Retornar HRNs de los grupos a los que pertenece
///         self.group_hrns.clone()
///     }
/// }
/// ```
pub trait HodeiEntity: std::fmt::Debug + Send + Sync {
    /// Retorna el HRN (Hodei Resource Name) canónico de esta entidad
    ///
    /// El HRN es el identificador único y global de la entidad.
    fn hrn(&self) -> &Hrn;

    /// Retorna los atributos de esta entidad como mapa clave-valor
    ///
    /// Los atributos son propiedades adicionales de la entidad que pueden
    /// usarse en la evaluación de políticas.
    fn attributes(&self) -> HashMap<AttributeName, AttributeValue>;

    /// Retorna los HRNs de las entidades parent (jerarquía/membership)
    ///
    /// Por ejemplo, un User puede retornar los HRNs de los Groups a los que pertenece.
    /// Por defecto retorna un vector vacío (sin parents).
    fn parent_hrns(&self) -> Vec<Hrn> {
        Vec::new()
    }

    /// Retorna los atributos de esta entidad en formato compatible con Cedar
    ///
    /// Esta es una extensión opcional del trait que permite a las entidades
    /// proporcionar sus atributos en un formato que Cedar puede entender directamente.
    /// Por defecto, convierte los atributos estándar a un formato compatible.
    fn cedar_attributes(&self) -> Option<Vec<(String, crate::domain::AttributeType)>> {
        // Convertir los atributos estándar a formato Cedar
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
                            AttributeValue::Set(_) => crate::domain::AttributeType::string(), // Anidado, usar String
                            AttributeValue::Record(_) => crate::domain::AttributeType::string(),
                            AttributeValue::EntityRef(_) => crate::domain::AttributeType::string(),
                        };
                        crate::domain::AttributeType::set(element_type)
                    } else {
                        crate::domain::AttributeType::set(crate::domain::AttributeType::string())
                    }
                }
                AttributeValue::Record(_) => crate::domain::AttributeType::string(), // Simplificado
                AttributeValue::EntityRef(_) => crate::domain::AttributeType::string(), // Simplificado
            };
            cedar_attrs.push((name.as_str().to_string(), cedar_type));
        }
        Some(cedar_attrs)
    }
}

// ============================================================================
// Marker Traits para roles en políticas
// ============================================================================

/// Marker trait para entidades que pueden actuar como Principal
///
/// Un Principal es la entidad que realiza una acción (ej: User, ServiceAccount).
/// Este trait requiere que la entidad implemente tanto `HodeiEntity` (runtime)
/// como `HodeiEntityType` (metadata).
pub trait Principal: HodeiEntity + HodeiEntityType {}

/// Marker trait para entidades que pueden actuar como Resource
///
/// Un Resource es la entidad sobre la que se realiza una acción.
/// Este trait requiere que la entidad implemente tanto `HodeiEntity` (runtime)
/// como `HodeiEntityType` (metadata).
pub trait Resource: HodeiEntity + HodeiEntityType {}

// ============================================================================
// ActionTrait - Define acciones del dominio
// ============================================================================

/// Define una acción que puede realizarse en el sistema
///
/// Las acciones son operaciones que un Principal puede realizar sobre un Resource.
///
/// # Ejemplo
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
    /// Nombre identificador único de la acción
    ///
    /// Por ejemplo: "CreateUser", "DeleteGroup", "AttachPolicy"
    fn name() -> &'static str;

    /// Nombre del servicio al que pertenece esta acción
    fn service_name() -> ServiceName;

    /// Nombre completo de la acción (Servicio::Action::"Nombre")
    ///
    /// Implementación por defecto que combina service_name y name.
    fn action_name() -> String {
        let service = Self::service_name();
        let namespace = crate::domain::hrn::Hrn::to_pascal_case(service.as_str());
        format!("{}::Action::\"{}\"", namespace, Self::name())
    }

    /// Tipo de Principal que puede realizar esta acción
    ///
    /// Retorna el nombre completo del tipo (ej: "Iam::User")
    fn applies_to_principal() -> String;

    /// Tipo de Resource sobre el que se puede realizar esta acción
    ///
    /// Retorna el nombre completo del tipo (ej: "Iam::Group")
    fn applies_to_resource() -> String;
}

// ============================================================================
// PolicyStorage - Abstracción de persistencia de políticas
// ============================================================================

/// Abstracción para almacenar y recuperar políticas
///
/// Este trait define la interfaz para persistir políticas como strings.
/// Las políticas se almacenan en su formato textual (ej: Cedar DSL, Rego, etc.)
/// sin acoplar el kernel a ningún formato específico.
#[async_trait::async_trait]
pub trait PolicyStorage: Send + Sync {
    /// Guarda una política
    ///
    /// # Parámetros
    /// - `id`: Identificador único de la política
    /// - `policy_text`: El texto de la política en su formato DSL
    async fn save_policy(&self, id: &str, policy_text: &str) -> Result<(), PolicyStorageError>;

    /// Elimina una política por su ID
    ///
    /// Retorna `true` si la política existía y fue eliminada, `false` si no existía.
    async fn delete_policy(&self, id: &str) -> Result<bool, PolicyStorageError>;

    /// Recupera una política por su ID
    ///
    /// Retorna `None` si la política no existe.
    async fn get_policy_by_id(&self, id: &str) -> Result<Option<String>, PolicyStorageError>;

    /// Carga todas las políticas almacenadas
    ///
    /// Retorna una lista de tuplas (id, policy_text)
    async fn load_all_policies(&self) -> Result<Vec<(String, String)>, PolicyStorageError>;
}

/// Errores de la capa de persistencia de políticas
#[derive(thiserror::Error, Debug)]
pub enum PolicyStorageError {
    /// Error genérico del proveedor de almacenamiento
    #[error("Underlying storage error: {0}")]
    ProviderError(#[from] Box<dyn std::error::Error + Send + Sync>),

    /// Error al parsear una política
    #[error("Policy parsing error: {0}")]
    ParsingError(String),

    /// Política no encontrada
    #[error("Policy not found: {0}")]
    PolicyNotFound(String),

    /// Error de validación
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
    // Tests de AttributeType
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
    // Tests de HodeiEntityType
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
    // Tests de HodeiEntity
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
    // Tests de ActionTrait
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
        assert_eq!(action_name, "Iam::Action::\"CreateUser\"");
    }

    #[test]
    fn action_trait_applies_to() {
        assert_eq!(CreateUserAction::applies_to_principal(), "Iam::User");
        assert_eq!(CreateUserAction::applies_to_resource(), "Iam::User");
    }

    // ========================================================================
    // Tests de PolicyStorage
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
    // Tests adicionales para AttributeType::Record
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
    // Tests para parent_types en HodeiEntityType
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
    // Tests para parent_hrns en HodeiEntity
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
    // Tests para Principal y Resource marker traits
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

        // Verificar que implementa HodeiEntity
        assert_eq!(principal.hrn().resource_id(), "test-principal");
        assert_eq!(TestPrincipal::is_principal_type(), true);
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

        // Verificar que implementa HodeiEntity
        assert_eq!(resource.hrn().resource_id(), "test-resource");
        assert_eq!(TestResource::is_resource_type(), true);
    }

    // ========================================================================
    // Tests para is_resource_type con valor por defecto
    // ========================================================================

    #[test]
    fn entity_type_is_resource_by_default() {
        // TestUser no sobrescribe is_resource_type, debería ser true por defecto
        assert_eq!(TestUser::is_resource_type(), true);
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

        // is_principal_type() debería ser false por defecto
        assert_eq!(DefaultEntity::is_principal_type(), false);
        // is_resource_type() debería ser true por defecto
        assert_eq!(DefaultEntity::is_resource_type(), true);
    }

    // ========================================================================
    // Tests adicionales para PolicyStorage con errores
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
                std::io::Error::new(std::io::ErrorKind::Other, "Provider error"),
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

        // Guardar política inicial
        storage.save_policy("p1", "version 1").await.unwrap();
        let v1 = storage.get_policy_by_id("p1").await.unwrap();
        assert_eq!(v1, Some("version 1".to_string()));

        // Actualizar con nueva versión
        storage.save_policy("p1", "version 2").await.unwrap();
        let v2 = storage.get_policy_by_id("p1").await.unwrap();
        assert_eq!(v2, Some("version 2".to_string()));
    }

    // ========================================================================
    // Tests para attributes_schema por defecto
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
