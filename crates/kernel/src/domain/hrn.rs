use serde::{Deserialize, Serialize};
use std::fmt;

/// Hrn (Hodei Resource Name)
///
/// Formato inspirado en ARN de AWS con la siguiente convención:
/// hrn:<partition>:<service>::<account_id>:<resource_type>/<resource_id>
///
/// Ejemplo:
/// hrn:aws:iam::123456789012:User/alice
///
/// Notas:
/// - El segmento de región se omite (doble `::`)
/// - `service` actúa como namespace lógico (se normaliza a lowercase)
/// - `resource_type` puede mapear a un tipo Cedar namespaced (ServicePascalCase::Type)
#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct Hrn {
    pub partition: String,
    pub service: String,
    pub account_id: String,
    pub resource_type: String,
    pub resource_id: String,
}

impl Hrn {
    /// Acceso al campo service
    pub fn service(&self) -> &str {
        &self.service
    }

    /// Acceso al campo resource_id
    pub fn resource_id(&self) -> &str {
        &self.resource_id
    }

    /// Acceso al campo resource_type
    pub fn resource_type(&self) -> &str {
        &self.resource_type
    }

    /// Acceso al campo partition
    pub fn partition(&self) -> &str {
        &self.partition
    }

    /// Acceso al campo account_id
    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    /// Convención: nombre de servicio siempre en minúsculas (puede contener dígitos y '-')
    fn normalize_service_name(service: &str) -> String {
        service.to_ascii_lowercase()
    }

    /// Convierte 'iam' o 'my-service' a 'Iam' o 'MyService' (namespace Cedar PascalCase)
    pub fn to_pascal_case(s: &str) -> String {
        s.split(['-', '_'])
            .filter(|seg| !seg.is_empty())
            .map(|seg| {
                let mut chars = seg.chars();
                match chars.next() {
                    Some(f) => {
                        f.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn new(
        partition: String,
        service: String,
        account_id: String,
        resource_type: String,
        resource_id: String,
    ) -> Self {
        Self {
            partition,
            service: Self::normalize_service_name(&service),
            account_id,
            resource_type,
            resource_id,
        }
    }

    /// Constructor usando un tipo que implemente `HodeiEntityType` para garantizar consistencia
    ///
    /// # Ejemplo
    /// ```ignore
    /// let user_hrn = Hrn::for_entity_type::<UserType>(
    ///     "hodei".to_string(),
    ///     "default".to_string(),
    ///     "user-123".to_string(),
    /// );
    /// ```
    pub fn for_entity_type<T: crate::domain::entity::HodeiEntityType>(
        partition: String,
        account_id: String,
        resource_id: String,
    ) -> Self {
        let service_name = T::service_name();
        let resource_type_name = T::resource_type_name();
        Self {
            partition,
            service: Self::normalize_service_name(service_name.as_str()),
            account_id,
            resource_type: resource_type_name.as_str().to_string(),
            resource_id,
        }
    }

    /// Parse HRN desde su representación en string
    pub fn from_string(hrn_str: &str) -> Option<Self> {
        let parts: Vec<&str> = hrn_str.split(':').collect();
        if parts.len() != 6 || parts[0] != "hrn" {
            return None;
        }

        let resource_parts: Vec<&str> = parts[5].splitn(2, '/').collect();
        if resource_parts.len() != 2 {
            return None;
        }

        Some(Hrn {
            partition: parts[1].to_string(),
            service: Self::normalize_service_name(parts[2]),
            account_id: parts[4].to_string(), // (region) se omite
            resource_type: resource_parts[0].to_string(),
            resource_id: resource_parts[1].to_string(),
        })
    }

    /// Construye el nombre completo del tipo de entidad (Namespace::Type)
    ///
    /// Este método es útil para construir identificadores de entidad
    /// para sistemas de políticas.
    ///
    /// Regla:
    /// - Si `resource_type` ya contiene `::`, se usa tal cual.
    /// - Sino y existe `service`, se produce `<ServicePascalCase>::<NormalizedResourceType>`
    pub fn entity_type_name(&self) -> String {
        let namespace = Self::to_pascal_case(&self.service);
        if self.resource_type.contains("::") {
            self.resource_type.clone()
        } else if !namespace.is_empty() {
            format!(
                "{}::{}",
                namespace,
                Self::normalize_ident(&self.resource_type)
            )
        } else {
            Self::normalize_ident(&self.resource_type)
        }
    }

    /// Construye un identificador de entidad completo en formato string
    ///
    /// Formato: `<EntityTypeName>::"<resource_id>"`
    /// Ejemplo: `Iam::User::"alice"`
    pub fn entity_uid_string(&self) -> String {
        let type_name = self.entity_type_name();
        format!("{}::\"{}\"", type_name, self.resource_id)
    }

    /// Normaliza un identificador para Cedar
    /// - Primer caracter debe ser [A-Za-z_] (sino se sustituye por '_')
    /// - El resto: alfanumérico o '_' (sino se sustituye por '_')
    fn normalize_ident(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        if let Some(c0) = chars.next() {
            let c = if c0.is_ascii_alphabetic() || c0 == '_' {
                c0
            } else {
                '_'
            };
            out.push(c);
        } else {
            out.push('_');
        }
        for c in chars {
            if c.is_ascii_alphanumeric() || c == '_' {
                out.push(c);
            } else {
                out.push('_');
            }
        }
        out
    }

    /// Constructor de conveniencia para acciones (`Action::"name"`)
    pub fn action(service: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            partition: "aws".to_string(),
            service: Self::normalize_service_name(&service.into()),
            account_id: String::new(),
            resource_type: "Action".to_string(),
            resource_id: name.into(),
        }
    }
}

impl fmt::Display for Hrn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "hrn:{}:{}::{}:{}/{}",
            self.partition, self.service, self.account_id, self.resource_type, self.resource_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display_hrn_roundtrip() {
        let s = "hrn:aws:hodei::123456789012:User/alice";
        let hrn = Hrn::from_string(s).expect("parse hrn");
        assert_eq!(hrn.partition, "aws");
        assert_eq!(hrn.service, "hodei");
        assert_eq!(hrn.account_id, "123456789012");
        assert_eq!(hrn.resource_type, "User");
        assert_eq!(hrn.resource_id, "alice");
        let rendered = hrn.to_string();
        assert!(rendered.contains("User/alice"));
    }

    #[test]
    fn entity_type_name_is_constructed() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let type_name = hrn.entity_type_name();
        assert_eq!(type_name, "Hodei::User");
    }

    #[test]
    fn entity_uid_string_format() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let uid_str = hrn.entity_uid_string();
        assert_eq!(uid_str, "Iam::User::\"alice\"");
    }

    #[test]
    fn entity_type_name_uses_service_namespace() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "hodei-svc".to_string(),
            "123".to_string(),
            "User-Profile".to_string(),
            "bob".to_string(),
        );
        let type_name = hrn.entity_type_name();
        assert_eq!(type_name, "HodeiSvc::User_Profile");
    }

    #[test]
    fn entity_type_name_uses_pascal_namespace() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let type_name = hrn.entity_type_name();
        assert_eq!(type_name, "Iam::User");
    }

    #[test]
    fn action_constructor_builds_action_type() {
        let hrn = Hrn::action("iam", "CreateUser");
        assert_eq!(hrn.resource_type, "Action");
        let type_name = hrn.entity_type_name();
        assert!(type_name.contains("Iam::Action"));
    }

    #[test]
    fn accessor_methods() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        assert_eq!(hrn.service(), "iam");
        assert_eq!(hrn.resource_id(), "alice");
        assert_eq!(hrn.resource_type(), "User");
        assert_eq!(hrn.partition(), "aws");
        assert_eq!(hrn.account_id(), "123456");
    }
}
