//! Typed schema assembler: builds Cedar schema fragments from HodeiEntityType metadata

use cedar_policy::{SchemaError, SchemaFragment};
use std::fmt::Write as _;
use crate::shared::HodeiEntityType;

/// Generate a Cedar SchemaFragment for a given entity type `T`.
///
/// Prefers typed metadata (cedar_attributes, is_principal_type).
pub fn generate_fragment_for_type<T: HodeiEntityType>() -> Result<SchemaFragment, Box<SchemaError>> {
    let attrs = T::cedar_attributes();

    let mut s = String::new();
    let ty = T::entity_type_name();
    let principal_clause = if T::is_principal_type() { " in Principal" } else { "" };

    // entity Header
    let _ = writeln!(s, "entity {}{} {{", ty, principal_clause);

    for (name, atype) in attrs {
        let _ = writeln!(s, "    {}: {}", name, atype.to_cedar_decl());
    }
    // Close entity
    s.push_str("};\n");

    // Build fragment
    let (frag, _warnings) = SchemaFragment::from_cedarschema_str(&s)
        .expect("typed fragment generation should parse");
    Ok(frag)
}
