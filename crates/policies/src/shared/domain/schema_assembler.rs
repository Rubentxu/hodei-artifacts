//! Typed schema assembler: builds Cedar schema fragments from HodeiEntityType metadata

use crate::shared::HodeiEntityType;
use cedar_policy::{Schema, SchemaError, SchemaFragment};
use std::fmt::Write as _;

fn is_lowercase(s: &str) -> bool { s.chars().all(|c| !c.is_ascii_alphabetic() || c.is_ascii_lowercase()) }
fn is_pascal_case(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() { Some(c0) if c0.is_ascii_uppercase() => {}, _ => return false }
    s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn invalid_schema_error() -> Box<SchemaError> {
    // Intenta construir un esquema vacío para obtener un SchemaError genérico
    Box::new(Schema::from_schema_fragments(vec![]).unwrap_err())
}

/// Generate a Cedar SchemaFragment for a given entity type `T`.
///
/// Uses the new service_name() and resource_type_name() methods to construct
/// the fully qualified entity type name (e.g., "IAM::User").
pub fn generate_fragment_for_type<T: HodeiEntityType>() -> Result<SchemaFragment, Box<SchemaError>>
{
    // Validación de convenciones
    let service = T::service_name();
    let resource = T::resource_type_name();
    if !is_lowercase(service) { return Err(invalid_schema_error()); }
    if !is_pascal_case(resource) { return Err(invalid_schema_error()); }

    let attrs = T::cedar_attributes();

    let mut s = String::new();
    let ty = T::cedar_entity_type_name();
    let principal_clause = if T::is_principal_type() {
        " in Principal"
    } else {
        ""
    };

    // entity Header
    let _ = writeln!(s, "entity {}{} {{", ty, principal_clause);

    for (i, (name, atype)) in attrs.iter().enumerate() {
        if i < attrs.len() - 1 {
            let _ = writeln!(s, "    {}: {},", name, atype.to_cedar_decl());
        } else {
            let _ = writeln!(s, "    {}: {}", name, atype.to_cedar_decl());
        }
    }
    // Close entity
    s.push_str("};\n");

    // Build fragment
    let (frag, _warnings) =
        SchemaFragment::from_cedarschema_str(&s).expect("typed fragment generation should parse");
    Ok(frag)
}
