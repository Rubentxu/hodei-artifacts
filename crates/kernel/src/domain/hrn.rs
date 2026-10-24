use serde::{Deserialize, Serialize};
use std::fmt;

/// Hrn (Hodei Resource Name)
///
/// Formato canónico (HRN v2):
/// `hrn:<partition>:<service>:<region>:<account_id>:<resource_type>/<resource_id>`
///
/// Ejemplo:
/// `hrn:hodei:artifacts:eu-west-1:acme:repository/releases`
///
/// Compatibilidad (HRN v1, alias):
/// `hrn:<partition>:<service>::<account_id>:<resource_type>/<resource_id>`
///
/// El HRN v1 es exactamente un HRN v2 con la **región vacía**. Se conserva
/// como alias: se parsea igual y se vuelve a serializar byte a byte idéntico,
/// de modo que ningún HRN almacenado cambia de forma.
///
/// Notas:
/// - `partition` nunca se infiere: un valor ausente o desconocido no se rellena
/// - `service` actúa como namespace lógico (se normaliza a lowercase)
/// - `resource_type` puede mapear a un tipo Cedar namespaced (ServicePascalCase::Type)
#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct Hrn {
    pub partition: String,
    pub service: String,
    /// Región del recurso. Vacía en los HRN v1 (alias) y en los recursos
    /// globales. Su ausencia se tolera al leer para que los HRN v1 persistidos
    /// sigan siendo legibles.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub region: String,
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

    /// Acceso al campo region
    ///
    /// Vacía en los HRN v1 (alias) y en los recursos globales.
    pub fn region(&self) -> &str {
        &self.region
    }

    /// Acceso al campo account_id
    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    /// ¿Este HRN es un alias v1 (sin región)?
    ///
    /// Un HRN v1 y un HRN v2 con la región vacía son la misma cosa: no hay
    /// ningún otro elemento que los distinga.
    pub fn is_legacy_v1(&self) -> bool {
        self.region.is_empty()
    }

    /// ¿Este HRN denota una acción y no un recurso?
    ///
    /// Las acciones son verbos, no recursos: no llevan scope de cuenta.
    pub fn is_action(&self) -> bool {
        self.resource_type == "Action"
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

    /// Construye un HRN v1 (alias): la región queda vacía.
    pub fn new(
        partition: String,
        service: String,
        account_id: String,
        resource_type: String,
        resource_id: String,
    ) -> Self {
        Self::with_region(
            partition,
            service,
            String::new(),
            account_id,
            resource_type,
            resource_id,
        )
    }

    /// Construye un HRN v2 con región explícita.
    ///
    /// Una región vacía produce un HRN v1 (alias), indistinguible de él.
    pub fn with_region(
        partition: String,
        service: String,
        region: String,
        account_id: String,
        resource_type: String,
        resource_id: String,
    ) -> Self {
        Self {
            partition,
            service: Self::normalize_service_name(&service),
            region,
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
            region: String::new(),
            account_id,
            resource_type: resource_type_name.as_str().to_string(),
            resource_id,
        }
    }

    /// Parse HRN desde su representación en string
    ///
    /// Acepta tanto el HRN v2 como su alias v1 (región vacía). Cualquier otra
    /// forma se rechaza.
    pub fn from_string(hrn_str: &str) -> Option<Self> {
        let parts: Vec<&str> = hrn_str.split(':').collect();
        if parts.len() != 6 || parts[0] != "hrn" {
            return None;
        }

        let resource_parts: Vec<&str> = parts[5].splitn(2, '/').collect();
        if resource_parts.len() != 2 {
            return None;
        }

        if parts[1].is_empty() || parts[2].is_empty() || parts[4].is_empty() {
            return None;
        }

        // The resource type and the resource id are both mandatory: an HRN
        // without them does not identify anything, so accepting it would
        // create identities that collide with each other.
        if resource_parts[0].is_empty() || resource_parts[1].is_empty() {
            return None;
        }

        Some(Hrn {
            partition: parts[1].to_string(),
            service: Self::normalize_service_name(parts[2]),
            region: parts[3].to_string(),
            account_id: parts[4].to_string(),
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
    /// - Sino y existe `service`, se produce `<ServicePascalCase>::<ResourceType>`
    ///
    /// El namespace es el **servicio** declarado por el tipo de entidad
    /// (`HodeiEntityType::entity_type_name()` usa exactamente la misma regla:
    /// `iam` + `User` → `Iam::User`), y el nombre del tipo es el tipo de
    /// recurso declarado, con la primera letra en mayúscula porque Cedar exige
    /// que un tipo de entidad empiece por letra mayúscula. Así, el tipo
    /// derivado de un HRN coincide con el tipo que la plataforma registra en el
    /// schema Cedar, y una política escrita contra el schema registrado nombra
    /// a la misma entidad que evalúa el motor.
    pub fn entity_type_name(&self) -> String {
        let namespace = Self::to_pascal_case(&self.service);
        if self.resource_type.contains("::") {
            self.resource_type.clone()
        } else {
            let entity_name = Self::to_cedar_type_ident(&self.resource_type);
            if namespace.is_empty() {
                entity_name
            } else {
                format!("{}::{}", namespace, entity_name)
            }
        }
    }

    /// Identificador opaco y completamente cualificado del recurso
    ///
    /// Es el HRN canónico completo. Dos recursos con el mismo `resource_id`
    /// pero en cuentas, servicios, regiones o particiones distintas producen
    /// identificadores distintos, así que no pueden confundirse aguas abajo
    /// (por ejemplo, como identificador de entidad Cedar).
    ///
    /// # Ejemplo
    /// `hrn:aws:iam::123456789012:User/alice`
    pub fn scoped_entity_id(&self) -> String {
        self.to_string()
    }

    /// Construye un identificador de entidad completo en formato string
    ///
    /// Formato: `<EntityTypeName>::"<scoped id>"`
    /// Ejemplo: `Iam::User::"hrn:aws:iam::123456789012:User/alice"`
    ///
    /// El identificador incluye el scope completo, de modo que dos recursos
    /// homónimos en cuentas distintas no comparten UID.
    pub fn entity_uid_string(&self) -> String {
        let type_name = self.entity_type_name();
        format!("{}::\"{}\"", type_name, self.escaped_scoped_entity_id())
    }

    /// El identificador cualificado, escapado para la sintaxis de Cedar
    ///
    /// Cedar escapa `\` y `"` con una barra invertida dentro de los
    /// identificadores entrecomillados.
    fn escaped_scoped_entity_id(&self) -> String {
        let mut out = String::with_capacity(self.scoped_entity_id().len());
        for ch in self.scoped_entity_id().chars() {
            match ch {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                _ => out.push(ch),
            }
        }
        out
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

    /// Normaliza un tipo de recurso al identificador de tipo de Cedar
    ///
    /// Cedar exige que un tipo de entidad empiece por letra mayúscula, así
    /// que la primera letra se eleva y el resto del nombre se conserva (un
    /// tipo compuesto como `UserProfile` no se decompone en `Userprofile`).
    fn to_cedar_type_ident(s: &str) -> String {
        let normalized = Self::normalize_ident(s);
        let mut chars = normalized.chars();
        match chars.next() {
            Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
            None => String::new(),
        }
    }

    /// Constructor de conveniencia para acciones (`Action::"name"`)
    ///
    /// Una acción no lleva scope de cuenta: su identificador Cedar es el
    /// nombre plano de la acción en el namespace `Action`.
    pub fn action(service: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            partition: "aws".to_string(),
            service: Self::normalize_service_name(&service.into()),
            region: String::new(),
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
            "hrn:{}:{}:{}:{}:{}/{}",
            self.partition,
            self.service,
            self.region,
            self.account_id,
            self.resource_type,
            self.resource_id
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

    /// Un HRN v1 es un HRN v2 con la región vacía: se serializa byte a byte
    /// idéntico, de modo que ningún HRN ya almacenado cambia de forma.
    #[test]
    fn a_v1_hrn_roundtrips_byte_identically() {
        let original = "hrn:aws:hodei::123456789012:User/alice";
        let hrn = Hrn::from_string(original).expect("parse hrn");

        assert_eq!(hrn.region(), "");
        assert!(hrn.is_legacy_v1());
        assert_eq!(hrn.to_string(), original);
    }

    /// Un HRN v2 conserva su región y solo se diferencia del v1 en ese campo.
    #[test]
    fn a_v2_hrn_roundtrips_with_its_region() {
        let original = "hrn:hodei:artifacts:eu-west-1:acme:repository/releases";
        let hrn = Hrn::from_string(original).expect("parse hrn");

        assert_eq!(hrn.partition, "hodei");
        assert_eq!(hrn.service, "artifacts");
        assert_eq!(hrn.region(), "eu-west-1");
        assert_eq!(hrn.account_id(), "acme");
        assert_eq!(hrn.resource_type(), "repository");
        assert_eq!(hrn.resource_id(), "releases");
        assert!(!hrn.is_legacy_v1());
        assert_eq!(hrn.to_string(), original);
    }

    /// Un HRN con región vacía no se distingue del v1: ambos son el mismo alias.
    #[test]
    fn an_empty_region_is_the_v1_alias() {
        let with_region = Hrn::with_region(
            "aws".to_string(),
            "iam".to_string(),
            String::new(),
            "123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        assert!(with_region.is_legacy_v1());
        assert_eq!(with_region.to_string(), "hrn:aws:iam::123:User/alice");
    }

    /// El identificador cualificado distingue dos cuentas distintas con el
    /// mismo nombre de recurso: es el requisito de aislamiento inter-tenant.
    #[test]
    fn the_scoped_id_separates_homonyms_of_different_accounts() {
        let alice_in_a = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "account-a".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        let alice_in_b = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "account-b".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        assert_ne!(alice_in_a.scoped_entity_id(), alice_in_b.scoped_entity_id());
        assert_ne!(
            alice_in_a.entity_uid_string(),
            alice_in_b.entity_uid_string()
        );
    }

    /// La región también forma parte del scope: el mismo nombre en dos
    /// regiones son dos recursos distintos.
    #[test]
    fn the_scoped_id_separates_homonyms_of_different_regions() {
        let in_eu = Hrn::with_region(
            "hodei".to_string(),
            "artifacts".to_string(),
            "eu-west-1".to_string(),
            "acme".to_string(),
            "repository".to_string(),
            "releases".to_string(),
        );
        let in_us = Hrn::with_region(
            "hodei".to_string(),
            "artifacts".to_string(),
            "us-east-1".to_string(),
            "acme".to_string(),
            "repository".to_string(),
            "releases".to_string(),
        );

        assert_ne!(in_eu.scoped_entity_id(), in_us.scoped_entity_id());
    }

    #[test]
    fn a_malformed_hrn_is_rejected() {
        assert!(Hrn::from_string("not-an-hrn").is_none());
        assert!(Hrn::from_string("hrn:aws:iam:123:User/alice").is_none());
        assert!(Hrn::from_string("hrn:aws:iam::123:User").is_none());
        assert!(Hrn::from_string("hrn:aws:iam:::User/alice").is_none());
        assert!(Hrn::from_string("hrn:aws:iam::123:User/").is_none());
    }

    /// El tipo derivado del HRN coincide con el tipo **declarado** que la
    /// plataforma registra en el schema Cedar: una política escrita contra el
    /// schema registrado nombra a la misma entidad que evalúa el motor.
    #[test]
    fn the_hrn_type_name_matches_the_declared_type_name() {
        use crate::domain::entity::HodeiEntityType;
        use crate::domain::{ResourceTypeName, ServiceName};

        struct DeclaredUser;

        impl HodeiEntityType for DeclaredUser {
            fn service_name() -> ServiceName {
                ServiceName::new("iam").unwrap()
            }

            fn resource_type_name() -> ResourceTypeName {
                ResourceTypeName::new("User").unwrap()
            }
        }

        let hrn = Hrn::for_entity_type::<DeclaredUser>(
            "aws".to_string(),
            "123".to_string(),
            "alice".to_string(),
        );

        assert_eq!(hrn.entity_type_name(), "Iam::User");
        assert_eq!(hrn.entity_type_name(), DeclaredUser::entity_type_name());
    }

    /// El tipo de recurso se normaliza a la forma que Cedar exige: primera
    /// letra en mayúscula, resto conservado (un tipo compuesto no se
    /// decompone).
    #[test]
    fn the_resource_type_is_normalized_to_a_cedar_type_name() {
        let lower = Hrn::new(
            "hodei".to_string(),
            "artifacts".to_string(),
            "acme".to_string(),
            "repository".to_string(),
            "releases".to_string(),
        );
        let composed = Hrn::new(
            "hodei".to_string(),
            "artifacts".to_string(),
            "acme".to_string(),
            "UserProfile".to_string(),
            "bob".to_string(),
        );

        assert_eq!(lower.entity_type_name(), "Artifacts::Repository");
        assert_eq!(composed.entity_type_name(), "Artifacts::UserProfile");
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
        assert_eq!(uid_str, "Iam::User::\"hrn:aws:iam::123:User/alice\"");
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
        assert!(hrn.is_action());
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
        assert_eq!(hrn.region(), "");
    }

    /// Un HRN persistido antes de la región debe seguir leyéndose, y un HRN
    /// con región no debe alterar el JSON de uno que no la tiene.
    #[test]
    fn a_persisted_v1_hrn_is_still_readable() {
        let v1 = r#"{"partition":"aws","service":"iam","account_id":"123","resource_type":"User","resource_id":"alice"}"#;
        let parsed: Hrn = serde_json::from_str(v1).expect("a v1 hrn must still deserialize");

        assert_eq!(parsed.region(), "");
        assert_eq!(parsed.to_string(), "hrn:aws:iam::123:User/alice");

        let serialized = serde_json::to_string(&parsed).expect("serialize");
        assert!(
            !serialized.contains("region"),
            "a v1 hrn must not gain a region field: {serialized}"
        );
    }
}
