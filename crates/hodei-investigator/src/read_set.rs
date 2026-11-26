//! The investigator's read-set.
//!
//! The primitive lives in [`hodei_evidence::domain::read_set`] because it is
//! shared DOMAIN: the investigator uses it to seal what it read
//! and the simulation uses it to pin the base on which it evaluates. This
//! module is the investigator's entry point and defines nothing:
//! it re-exports the same primitive, so that a manifest written by
//! one verifies that of the other.

pub use hodei_evidence::domain::read_set::{ReadKind, ReadSet, ReadStamp};
