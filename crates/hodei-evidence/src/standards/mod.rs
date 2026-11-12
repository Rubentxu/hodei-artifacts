//! The normalizers that read the standards into Hodei's facts.
//!
//! The reference standards enter Hodei through **local normalizers and
//! conformance fixtures**, before any vendor connector is commissioned.
//! That is the rule the plan sets, and this module is where it is carried
//! out.
//!
//! # What is here
//!
//! - [`inventory`] reads CycloneDX 1.7 and SPDX 3.0 into the same native
//!   model. The SPDX path is the compatibility path: not a second model,
//!   but a second normalizer that produces the same facts.
//! - [`attestation`] reads an in-toto statement and **verifies** it.
//! - [`dsse`] is the envelope and the verification of its signatures.
//! - [`correlation`] says when two names are the same thing, and how sure
//!   Hodei is of it.
//!
//! # The rule that shapes all of them
//!
//! **The uncertainty is visible.** A mapping that could not be confirmed
//! says so. An attestation that was not verified says so. The document is
//! normalized into facts, and the document itself is never rewritten.

pub mod attestation;
pub mod correlation;
pub mod dsse;
pub mod inventory;

pub use attestation::{
    ATTESTATION_NORMALIZER_VERSION, AttestationError, bind_to_subject, read_attestation,
};
pub use correlation::{CorrelationError, correlate_identities};
pub use dsse::{KeyRing, VerificationKey, VerificationOutcome, pae, verify_envelope};
pub use inventory::{
    CYCLONEDX_NORMALIZER_VERSION, InventoryError, SPDX_NORMALIZER_VERSION, read_cyclonedx_17,
    read_spdx_30,
};
