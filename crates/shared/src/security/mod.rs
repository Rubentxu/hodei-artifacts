// crates/shared/src/security/mod.rs

pub mod cedar_integration;
pub mod resources;

// Re-export para fácil acceso
pub use cedar_integration::{to_cedar_entity, CedarEntityConverter};
pub use resources::HodeiResource;

use std::collections::HashMap;

// /// Ejemplo de implementación para Organization usando tipos Cedar
// impl HodeiResource<cedar_policy::EntityUid, cedar_policy::Expr> for Organization {
//     fn resource_id(&self) -> cedar_policy::EntityUid {
//         cedar_policy::EntityUid::from_str(self.hrn.as_str()).unwrap()
//     }

//     fn resource_attributes(&self) -> HashMap<String, cedar_policy::Expr> {
//         let mut attrs = HashMap::new();
//         attrs.insert("type".to_string(), cedar_policy::Expr::val("organization"));
//         attrs.insert("status".to_string(), cedar_policy::Expr::val(self.status.to_string()));
//         attrs.insert("primary_region".to_string(), cedar_policy::Expr::val(self.primary_region.clone()));
//         attrs
//     }

//     fn resource_parents(&self) -> Vec<cedar_policy::EntityUid> {
//         Vec::new() // Organizaciones no tienen padres
//     }
// }
