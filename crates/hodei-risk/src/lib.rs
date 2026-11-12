//! The risk graph of Hodei: the vulnerabilities, what they affect, and the
//! case that tracks them.
//!
//! Hodei does not have a vulnerability database of its own and does not
//! scan. It reads what the producers publish --OSV for the advisories,
//! VEX for the statements that say what does not apply-- and it says when
//! it does not know something.
//!
//! # The rule that holds it all
//!
//! **The absence of evidence is not evidence of absence.** An artefact with
//! no inventory has an *unknown* state, never an *unaffected* one. A
//! version that cannot be compared is unknown, never unaffected. A VEX
//! that nobody verified does not silence a finding.
//!
//! A system that cannot say "nobody knows" ends up reporting that a
//! deployment with four hundred dependencies is fine, and that is how
//! people stop reading the reports.

#![forbid(unsafe_code)]

pub mod affected;
pub mod case;
pub mod osv;
pub mod traversal;
pub mod version;
pub mod vex;

pub use affected::{
    AffectedState, DependencyKind, Finding, VexReference, apply_vex, state_without_vex,
};
pub use case::{
    BlastRadius, CaseEvent, CaseEventKind, CaseState, SecurityCase, VerificationCriteria,
};
pub use osv::{
    OSV_SCHEMA_VERSION, OsvAdvisory, OsvAffected, OsvError, OsvEvent, OsvPackage, OsvRange,
    OsvReference, read_osv,
};
pub use traversal::{AffectedPath, Edge, affected_paths, blast_radius};
pub use version::{Match, match_version};
pub use vex::{VexError, VexJustification, VexStatement, VexStatus, read_vex};
