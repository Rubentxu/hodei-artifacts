use cedar_policy::{EntityId, EntityTypeName, EntityUid};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct Hrn {
    pub partition: String,
    pub service: String,
    pub account_id: String,
    pub resource_type: String,
    pub resource_id: String,
}

impl Hrn {
    /// Convención AWS: nombre de servicio siempre en minúsculas (puede contener dígitos y '-')
    pub fn normalize_service_name(service: &str) -> String {
        service.to_ascii_lowercase()
    }

    /// Convierte 'iam' o 'my-service' a 'Iam' o 'MyService' (namespace Cedar)
    pub fn to_pascal_case(s: &str) -> String {
        s.split(|c| c == '-' || c == '_')
            .filter(|seg| !seg.is_empty())
            .map(|seg| {
                let mut chars = seg.chars();
                match chars.next() {
                    Some(f) => f.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase(),
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

    /// Constructor usando HodeiEntityType para garantizar consistencia
    ///
    /// Este método construye un HRN usando la información del tipo, eliminando
    /// la posibilidad de desincronización entre el esquema y las instancias.
    ///
    /// # Ejemplo
    /// ```ignore
    /// use policies::shared::domain::hrn::Hrn;
    /// use hodei_iam::User; // From hodei-iam crate
    ///
    /// let user_hrn = Hrn::for_entity_type::<User>(
    ///     "hodei".to_string(),
    ///     "default".to_string(),
    ///     "user-123".to_string(),
    /// );
    /// ```
    pub fn for_entity_type<T: crate::shared::domain::ports::HodeiEntityType>(
        partition: String,
        account_id: String,
        resource_id: String,
    ) -> Self {
        Self {
            partition,
            service: Self::normalize_service_name(T::service_name()),
            account_id,
            resource_type: T::resource_type_name().to_string(),
            resource_id,
        }
    }

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
            account_id: parts[4].to_string(), // El 3er segmento (region) se omite
            resource_type: resource_parts[0].to_string(),
            resource_id: resource_parts[1].to_string(),
        })
    }

    /// Convert HRN to Cedar EntityUid con namespace PascalCase (p.ej., Iam::User)
    ///
    /// Cedar expects UIDs as `Type::"id"`, where Type may be namespaced like `App::User`.
    /// We map:
    /// - Type: if `resource_type` already contains `::`, it's used as-is.
    ///   otherwise, when `service` is non-empty we construct `"{service}::{resource_type}"`.
    ///   both components are normalized to valid Cedar identifiers.
    /// - Id: always quoted string; if parsing fails, we wrap in quotes.
    pub fn euid(&self) -> EntityUid {
        // Namespace Cedar con PascalCase derivado del servicio
        let namespace = Self::to_pascal_case(&self.service);
        let type_str = if self.resource_type.contains("::") {
            self.resource_type.clone()
        } else if !namespace.is_empty() {
            format!("{}::{}", namespace, Self::normalize_ident(&self.resource_type))
        } else {
            Self::normalize_ident(&self.resource_type)
        };

        let eid = EntityId::from_str(&self.resource_id)
            .or_else(|_| EntityId::from_str(&format!("\"{}\"", self.resource_id)))
            .expect("Failed to create EntityId");
        let type_name =
            EntityTypeName::from_str(&type_str).expect("Failed to create EntityTypeName");
        EntityUid::from_type_name_and_id(type_name, eid)
    }

    /// Normalize a free-form string into a Cedar identifier segment
    /// - first char must be [A-Za-z_]; others may include digits
    /// - non-conforming chars are replaced by '_'
    fn normalize_ident(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        if let Some(c0) = chars.next() {
            let c = if c0.is_ascii_alphabetic() || c0 == '_' { c0 } else { '_' };
            out.push(c);
        } else {
            out.push('_');
        }
        for c in chars {
            if c.is_ascii_alphanumeric() || c == '_' { out.push(c); } else { out.push('_'); }
        }
        out
    }

    /// Convenience constructor for Action identifiers. This creates an HRN that
    /// translates into an EntityUid of the form `<service>::Action::"name"` when
    /// `service` is provided, otherwise `Action::"name"`.
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
    fn euid_is_constructed() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let euid = hrn.euid();
        // Basic sanity: formatting should include type and id
        let s = format!("{}", euid);
        assert!(s.contains("User"));
        assert!(s.contains("alice"));
    }

    #[test]
    fn euid_uses_service_namespace_for_type() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "hodei-svc".to_string(),
            "123".to_string(),
            "User-Profile".to_string(),
            "bob".to_string(),
        );
        let euid = hrn.euid();
        let s = format!("{}", euid);
        // Expect PascalCase namespace and normalized type (guiones convertidos a guiones bajos)
        assert!(s.contains("HodeiSvc::User_Profile"));
        assert!(s.contains("\"bob\""));
    }

    #[test]
    fn euid_uses_pascal_namespace() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let euid = hrn.euid();
        let s = format!("{}", euid);
        assert!(s.contains("Iam::User"));
        assert!(s.contains("\"alice\""));
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
