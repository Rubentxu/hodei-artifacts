// Gate legacy modules behind feature flag during refactor
#[cfg(feature = "legacy_infra")]
pub mod entity_utils;

pub mod error;
pub mod policy;

#[deprecated(
    note = "Use `shared::domain::hrn::Hrn` instead. This module will be removed after migration."
)]
#[cfg(feature = "legacy_infra")]
pub mod hrn;

#[deprecated(
    note = "Use re-exports from the `shared` crate (shared::{Hrn,HodeiEntityType,...}). This module will be removed after migration."
)]
pub mod ports;

#[cfg(feature = "legacy_infra")]
pub mod schema_assembler;

pub use error::HodeiPoliciesError;
pub use policy::{Policy, PolicyId, PolicyMetadata};

#[allow(deprecated)]
#[cfg(feature = "legacy_infra")]
pub use hrn::Hrn;

#[allow(deprecated)]
pub use ports::{ActionTrait, AttributeType, HodeiEntity, HodeiEntityType, Principal, Resource};
