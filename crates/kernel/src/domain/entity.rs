use cedar_policy::{EntityTypeName, EntityUid, Policy, RestrictedExpression};
use std::collections::HashMap;
use std::str::FromStr;

/// Tipos de atributos para describir atributos de esquema Cedar de forma tipada.
#[derive(Debug, Clone)]
pub enum AttributeType {
    /// Atributo primitivo (p.ej. "String", "Long", "Bool")
    Primitive(&'static str),
    /// Conjunto de otro tipo (Set<T>)
    Set(Box<AttributeType>),
    /// Referencia a otra entidad por id (EntityId<Tipo>)
    EntityId(&'static str),
}

impl AttributeType {
    /// Devuelve la declaración Cedar textual (p.ej. "Set<String>")
    pub fn to_cedar_decl(&self) -> String {
        match self {
            AttributeType::Primitive(name) => name.to_string(),
            AttributeType::Set(inner) => format!("Set<{}>", inner.to_cedar_decl()),
            AttributeType::EntityId(entity_ty) => format!("EntityId<{}>", entity_ty),
        }
    }
}

/// Metadata a nivel de tipo para construir fragmentos de esquema Cedar.
///
/// Cada entidad del dominio que quiera integrarse con el motor de políticas
/// debe implementar este trait (así la generación de esquema es automática).
pub trait HodeiEntityType {
    /// Nombre del servicio (namespace lógico) - se normalizará a PascalCase en Cedar.
    fn service_name() -> &'static str;

    /// Nombre local del tipo de recurso (ej. "User", "Group", "Account").
    fn resource_type_name() -> &'static str;

    /// Nombre completo del entity type Cedar (Namespace::Tipo).
    fn cedar_entity_type_name() -> EntityTypeName {
        let namespace = crate::domain::hrn::Hrn::to_pascal_case(Self::service_name());
        let type_str = format!("{}::{}", namespace, Self::resource_type_name());
        EntityTypeName::from_str(&type_str)
            .expect("Failed to create EntityTypeName from service + resource type")
    }

    /// DEPRECATED: usar `cedar_entity_type_name` (se mantiene temporalmente por transición).
    #[allow(deprecated)]
    fn entity_type_name() -> &'static str {
        Self::resource_type_name()
    }

    /// Indica si este tipo puede actuar como Principal en políticas.
    fn is_principal_type() -> bool {
        false
    }

    /// Atributos declarados para generación de esquema.
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        Vec::new()
    }

    /// Tipos parent conceptuales para relaciones jerárquicas (opcional).
    fn cedar_parents_types() -> Vec<&'static str> {
        Vec::new()
    }
}

/// Representa una instancia concreta (runtime) de una entidad de dominio integrable con Cedar.
pub trait HodeiEntity {
    /// Referencia a su HRN canónico.
    fn hrn(&self) -> &crate::domain::hrn::Hrn;

    /// Atributos dinámicos en forma de mapa Cedar (RestrictedExpression).
    fn attributes(&self) -> HashMap<String, RestrictedExpression>;

    /// Padres (membership) expresados como EntityUids. Por defecto ninguno.
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }

    /// Convenience: obtener el EntityUid Cedar.
    fn euid(&self) -> EntityUid {
        self.hrn().to_euid()
    }
}

/// Marker trait para entidades que pueden actuar como Principal.
pub trait Principal: HodeiEntity + HodeiEntityType {}

/// Marker trait para entidades que pueden actuar como Resource.
pub trait Resource: HodeiEntity + HodeiEntityType {}

/// Define una Acción que puede registrarse en el motor de políticas.
///
/// Una acción se materializa como una entidad Cedar del tipo `<Service>::Action::"Nombre"`.
pub trait ActionTrait {
    /// Nombre identificador único de la acción (ej. "CreateUser").
    fn name() -> &'static str;

    /// Par (PrincipalType, ResourceType) al que aplica.
    fn applies_to() -> (EntityTypeName, EntityTypeName);
}

/// Abstracción de almacenamiento de políticas (persistencia).
#[async_trait::async_trait]
pub trait PolicyStorage: Send + Sync {
    async fn save_policy(&self, policy: &Policy) -> Result<(), PolicyStorageError>;
    async fn delete_policy(&self, id: &str) -> Result<bool, PolicyStorageError>;
    async fn get_policy_by_id(&self, id: &str) -> Result<Option<Policy>, PolicyStorageError>;
    async fn load_all_policies(&self) -> Result<Vec<Policy>, PolicyStorageError>;
}

/// Errores de la capa de persistencia de políticas.
#[derive(thiserror::Error, Debug)]
pub enum PolicyStorageError {
    #[error("Underlying storage error: {0}")]
    ProviderError(#[from] Box<dyn std::error::Error + Send + Sync>),

    #[error("Policy parsing error: {0}")]
    ParsingError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use cedar_policy::Policy;

    struct DummyPrincipalType;
    impl HodeiEntityType for DummyPrincipalType {
        fn service_name() -> &'static str {
            "iam"
        }
        fn resource_type_name() -> &'static str {
            "User"
        }
        fn is_principal_type() -> bool {
            true
        }
    }

    struct DummyEntityInstance {
        hrn: crate::domain::hrn::Hrn,
    }
    impl DummyEntityInstance {
        fn new(id: &str) -> Self {
            Self {
                hrn: crate::domain::hrn::Hrn::for_entity_type::<DummyPrincipalType>(
                    "aws".to_string(),
                    "123456789012".to_string(),
                    id.to_string(),
                ),
            }
        }
    }
    impl HodeiEntity for DummyEntityInstance {
        fn hrn(&self) -> &crate::domain::hrn::Hrn {
            &self.hrn
        }
        fn attributes(&self) -> HashMap<String, RestrictedExpression> {
            HashMap::new()
        }
    }
    // Removed impl Principal for DummyPrincipalType (it does not implement HodeiEntity)

    struct InMemoryPolicyStorage {
        items: std::sync::Mutex<HashMap<String, Policy>>,
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
        async fn save_policy(&self, policy: &Policy) -> Result<(), PolicyStorageError> {
            let mut guard = self.items.lock().unwrap();
            guard.insert(policy.id().to_string(), policy.clone());
            Ok(())
        }

        async fn delete_policy(&self, id: &str) -> Result<bool, PolicyStorageError> {
            let mut guard = self.items.lock().unwrap();
            Ok(guard.remove(id).is_some())
        }

        async fn get_policy_by_id(&self, id: &str) -> Result<Option<Policy>, PolicyStorageError> {
            let guard = self.items.lock().unwrap();
            Ok(guard.get(id).cloned())
        }

        async fn load_all_policies(&self) -> Result<Vec<Policy>, PolicyStorageError> {
            let guard = self.items.lock().unwrap();
            Ok(guard.values().cloned().collect())
        }
    }

    #[test]
    fn cedar_entity_type_name_builds() {
        let et = DummyPrincipalType::cedar_entity_type_name();
        assert_eq!(et.to_string(), "Iam::User");
    }

    #[test]
    fn entity_instance_hrn_to_euid() {
        let instance = DummyEntityInstance::new("alice");
        let euid = instance.euid();
        let s = format!("{euid}");
        assert!(s.contains("Iam::User"));
        assert!(s.contains("alice"));
    }

    #[tokio::test]
    async fn in_memory_policy_storage_roundtrip() {
        let storage = InMemoryPolicyStorage::new();
        // Policy mínima válida: policy allow if true;
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy =
            cedar_policy::Policy::parse(None, policy_src).expect("parse simple permit policy");
        storage.save_policy(&policy).await.unwrap();
        let loaded = storage
            .get_policy_by_id(&policy.id().to_string())
            .await
            .unwrap();
        assert!(loaded.is_some());
        let all = storage.load_all_policies().await.unwrap();
        assert_eq!(all.len(), 1);
    }
}
