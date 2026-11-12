//! The standards that Hodei reads, and the model they are read into.
//!
//! The reference standards enter Hodei through **local normalizers and
//! conformance fixtures**, before any vendor connector is commissioned. That
//! is the rule the plan sets, and this module is where it is carried out:
//! the model of Hodei's own facts, and the normalizers that produce them.
//!
//! # The three parts
//!
//! - [`identity`] says what a thing is called outside Hodei and how sure
//!   Hodei is of the mapping.
//! - [`sbom`] says what is inside a thing, in Hodei's own vocabulary.
//! - [`attestation`] says who claims a thing was built, and whether the
//!   claim was checked.
//!
//! # The rule that shapes all three
//!
//! **The uncertainty is visible.** A mapping that could not be confirmed
//! says so. An attestation that was not verified says so. A document is
//! normalized into facts, and the document itself is never rewritten.

pub mod attestation;
pub mod identity;
pub mod sbom;

pub use attestation::{
    Attestation, AttestationStatement, AttestationSubject, AttestationVerification, DsseEnvelope,
    DsseSignature, IN_TOTO_PAYLOAD_TYPE, IN_TOTO_STATEMENT_TYPE, SLSA_PROVENANCE_PREDICATE_TYPE,
    SignatureFailure, SlsaProvenance,
};
pub use identity::{
    CANONICALIZATION_VERSION, ExternalIdentity, IdentityNamespace, MappingConfidence,
};
pub use sbom::{ComponentKind, Sbom, SbomComponent, SbomDependency, SbomFormat, SbomSubject};
