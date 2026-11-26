use serde::{Deserialize, Serialize};
use std::fmt;

/// Hrn (Hodei Resource Name)
///
/// Canonical format (HRN v2):
/// `hrn:<partition>:<service>:<region>:<account_id>:<resource_type>/<resource_id>`
///
/// Example:
/// `hrn:hodei:artifacts:eu-west-1:acme:repository/releases`
///
/// Compatibility (HRN v1, alias):
/// `hrn:<partition>:<service>::<account_id>:<resource_type>/<resource_id>`
///
/// The HRN v1 is exactly an HRN v2 with an **empty region**. It is kept
/// as an alias: it parses the same and serializes back byte for byte identical,
/// so that no stored HRN changes shape.
///
/// Notes:
/// - `partition` is never inferred: an absent or unknown value is not filled in
/// - `service` acts as a logical namespace (normalized to lowercase)
/// - `resource_type` can map to a namespaced Cedar type (ServicePascalCase::Type)
#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct Hrn {
    pub partition: String,
    pub service: String,
    /// Region of the resource. Empty in HRN v1 (alias) and in global
    /// resources. Its absence is tolerated when reading so that persisted HRN v1
    /// remain readable.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub region: String,
    pub account_id: String,
    pub resource_type: String,
    pub resource_id: String,
}

impl Hrn {
    /// Access to the service field
    pub fn service(&self) -> &str {
        &self.service
    }

    /// Access to the resource_id field
    pub fn resource_id(&self) -> &str {
        &self.resource_id
    }

    /// Access to the resource_type field
    pub fn resource_type(&self) -> &str {
        &self.resource_type
    }

    /// Access to the partition field
    pub fn partition(&self) -> &str {
        &self.partition
    }

    /// Access to the region field
    ///
    /// Empty in HRN v1 (alias) and in global resources.
    pub fn region(&self) -> &str {
        &self.region
    }

    /// Access to the account_id field
    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    /// Is this HRN a v1 alias (without region)?
    ///
    /// An HRN v1 and an HRN v2 with an empty region are the same thing: there is
    /// no other element that tells them apart.
    pub fn is_legacy_v1(&self) -> bool {
        self.region.is_empty()
    }

    /// Convention: service name always lowercase (may contain digits and '-')
    fn normalize_service_name(service: &str) -> String {
        service.to_ascii_lowercase()
    }

    /// Converts 'iam' or 'my-service' to 'Iam' or 'MyService' (Cedar PascalCase namespace)
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

    /// Builds an HRN v1 (alias): the region is left empty.
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

    /// Builds an HRN v2 with explicit region.
    ///
    /// An empty region produces an HRN v1 (alias), indistinguishable from it.
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

    /// Constructor using a type that implements `HodeiEntityType` to guarantee consistency
    ///
    /// # Example
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

    /// Parse an HRN from its string representation
    ///
    /// Accepts both the HRN v2 and its v1 alias (empty region). Any other
    /// form is rejected.
    pub fn from_string(hrn_str: &str) -> Option<Self> {
        // Delegate resource parsing to the single canonical contract. Keeping
        // a second, more permissive parser here let the HTTP edge accept shapes
        // that authorization rejected later.
        let parsed = hodei_hrn::Hrn::from_string(hrn_str)?;
        Some(Self {
            partition: parsed.partition().to_string(),
            service: parsed.service().to_string(),
            region: parsed.region().to_string(),
            account_id: parsed.account().to_string(),
            resource_type: parsed.resource_type().to_string(),
            resource_id: parsed.resource_path().to_string(),
        })
    }

    /// Builds the full entity type name (Namespace::Type)
    ///
    /// This method is useful for building entity identifiers
    /// for policy systems.
    ///
    /// Rule:
    /// - If `resource_type` already contains `::`, it is used as is.
    /// - Otherwise, if `service` exists, `<ServicePascalCase>::<ResourceType>` is produced
    ///
    /// The namespace is the **service** declared by the entity type
    /// (`HodeiEntityType::entity_type_name()` uses exactly the same rule:
    /// `iam` + `User` → `Iam::User`), and the type name is the declared resource
    /// type, with the first letter uppercased because Cedar requires
    /// an entity type to start with an uppercase letter. This way, the type
    /// derived from an HRN matches the type that the platform registers in the
    /// Cedar schema, and a policy written against the registered schema names
    /// the same entity that the engine evaluates.
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

    /// Opaque and fully qualified identifier of the resource
    ///
    /// It is the full canonical HRN. Two resources with the same `resource_id`
    /// but in different accounts, services, regions or partitions produce
    /// different identifiers, so they cannot be confused downstream
    /// (for example, as a Cedar entity identifier).
    ///
    /// # Example
    /// `hrn:aws:iam::123456789012:User/alice`
    pub fn scoped_entity_id(&self) -> String {
        self.to_string()
    }

    /// Builds a full entity identifier in string format
    ///
    /// Format: `<EntityTypeName>::"<scoped id>"`
    /// Example: `Iam::User::"hrn:aws:iam::123456789012:User/alice"`
    ///
    /// The identifier includes the full scope, so that two homonymous resources
    /// in different accounts do not share a UID.
    pub fn entity_uid_string(&self) -> String {
        let type_name = self.entity_type_name();
        format!("{}::\"{}\"", type_name, self.escaped_scoped_entity_id())
    }

    /// The qualified identifier, escaped for Cedar syntax
    ///
    /// Cedar escapes `\` and `"` with a backslash inside
    /// quoted identifiers.
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

    /// Normalizes an identifier for Cedar
    /// - First character must be [A-Za-z_] (otherwise it is replaced by '_')
    /// - The rest: alphanumeric or '_' (otherwise it is replaced by '_')
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

    /// Normalizes a resource type to the Cedar type identifier
    ///
    /// Cedar requires an entity type to start with an uppercase letter, so
    /// the first letter is raised and the rest of the name is kept (a
    /// compound type such as `UserProfile` is not decomposed into `Userprofile`).
    fn to_cedar_type_ident(s: &str) -> String {
        let normalized = Self::normalize_ident(s);
        let mut chars = normalized.chars();
        match chars.next() {
            Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
            None => String::new(),
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

    /// An HRN v1 is an HRN v2 with an empty region: it serializes byte for byte
    /// identical, so that no already stored HRN changes shape.
    #[test]
    fn a_v1_hrn_roundtrips_byte_identically() {
        let original = "hrn:aws:hodei::123456789012:User/alice";
        let hrn = Hrn::from_string(original).expect("parse hrn");

        assert_eq!(hrn.region(), "");
        assert!(hrn.is_legacy_v1());
        assert_eq!(hrn.to_string(), original);
    }

    /// An HRN v2 keeps its region and differs from v1 only in that field.
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

    /// An HRN with an empty region is not distinguishable from v1: both are the same alias.
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

    /// The qualified identifier tells apart two different accounts with the
    /// same resource name: it is the inter-tenant isolation requirement.
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

    /// The region is also part of the scope: the same name in two
    /// regions are two different resources.
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

    /// The type derived from the HRN matches the **declared** type that the
    /// platform registers in the Cedar schema: a policy written against the
    /// registered schema names the same entity that the engine evaluates.
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

    /// The resource type is normalized to the form that Cedar requires: first
    /// letter uppercase, the rest kept (a compound type is not
    /// decomposed).
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

    /// An HRN persisted before the region must keep being readable, and an HRN
    /// with a region must not alter the JSON of one that does not have it.
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

#[test]
fn canonical_resource_parser_rejects_actions_and_lax_paths() {
    assert!(Hrn::from_string("hrn:hodei:api::default:Action/read").is_none());
    assert!(Hrn::from_string("hrn:hodei:artifacts:eu:acme:repository/releases//v2").is_none());
}
