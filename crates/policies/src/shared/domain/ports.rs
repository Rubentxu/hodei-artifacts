//! Legacy definitions moved to the shared kernel (`shared::domain`).
//! This module now acts purely as a compatibility re-export layer.
//!
//! Migration path:
//! - Replace imports like `policies::shared::domain::ports::HodeiEntityType`
//!   with `shared::HodeiEntityType` (re-exported at crate root of `shared`).
//!
//! This file will eventually be removed once all downstream crates
//! stop referencing it directly.

pub use kernel::{
    ActionTrait, AttributeType, HodeiEntity, HodeiEntityType, Hrn, PolicyStorage,
    PolicyStorageError as StorageError, Principal, Resource,
};
