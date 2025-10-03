//! Typed schema assembler: builds Cedar schema fragments from HodeiEntityType metadata

use crate::shared::HodeiEntityType;
use cedar_policy::{CedarSchemaError, SchemaFragment};
use std::fmt::Write as _;

fn is_lowercase(s: &str) -> bool { s.chars().all(|c| !c.is_ascii_alphabetic() || c.is_ascii_lowercase()) }
fn is_pascal_case(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() { Some(c0) if c0.is_ascii_uppercase() => {}, _ => return false }
    s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn invalid_schema_error() -> Box<CedarSchemaError> {
    // Genera un SchemaError intentando parsear un esquema inválido
    let invalid_schema = "entity Invalid { invalid_attr: InvalidType };";
    match SchemaFragment::from_cedarschema_str(invalid_schema) {
        Err(e) => Box::new(e),
        Ok(_) => {
            // Si por alguna razón el esquema inválido es válido, intentamos con otro
            let conflicting = r#"
                entity Test {};
                entity Test {};
            "#;
            match SchemaFragment::from_cedarschema_str(conflicting) {
                Err(e) => Box::new(e),
                Ok(_) => panic!("Failed to generate a SchemaError"),
            }
        }
    }
}

/// Generate a Cedar SchemaFragment for a given entity type `T`.
///
/// Uses the new service_name() and resource_type_name() methods to construct
/// the fully qualified entity type name (e.g., "IAM::User").
pub fn generate_fragment_for_type<T: HodeiEntityType>() -> Result<SchemaFragment, Box<CedarSchemaError>>
{
    // Validación de convenciones
    let service = T::service_name();
    let resource = T::resource_type_name();
    if !is_lowercase(service) { return Err(invalid_schema_error()); }
    if !is_pascal_case(resource) { return Err(invalid_schema_error()); }

    let attrs = T::cedar_attributes();

    let mut s = String::new();
    
    // Para entidades con namespace, necesitamos declarar el namespace primero
    let namespace = crate::shared::Hrn::to_pascal_case(service);
    let _ = writeln!(s, "namespace {} {{", namespace);
    
    // No usamos "in [Principal]" porque Principal debe estar definido globalmente
    // En su lugar, las entidades principales se identifican por su uso en las acciones

    // entity Header (sin el namespace, ya que estamos dentro del bloque namespace)
    let _ = writeln!(s, "    entity {} {{", resource);

    for (i, (name, atype)) in attrs.iter().enumerate() {
        if i < attrs.len() - 1 {
            let _ = writeln!(s, "        {}: {},", name, atype.to_cedar_decl());
        } else {
            let _ = writeln!(s, "        {}: {}", name, atype.to_cedar_decl());
        }
    }
    // Close entity
    let _ = writeln!(s, "    }};");
    // Close namespace
    let _ = writeln!(s, "}}");

    // Build fragment
    let (frag, _warnings) =
        SchemaFragment::from_cedarschema_str(&s).expect("typed fragment generation should parse");
    Ok(frag)
}
