//! The **raw** content surface.
//!
//! A raw repository has no packages, no versions, no formats:
//! it has **bytes at a path**. That is why the surface is small on
//! purpose: `PUT` publishes, `GET` downloads, `HEAD` describes, and all three
//! talk to a `hosted` repository.
//!
//! What this surface does **not** do:
//!
//! - It shares no semantics with OCI, Maven or npm: a repository of another
//!   format is not served here, and a path of another format means
//!   nothing here.
//! - It does not decide authorization on its own: it asks Cedar for the decision
//!   with the corresponding action, and if the decision is not permit, there is
//!   no content.
//! - It does not admit an ambiguous path: the rules are in
//!   [`path`], and are applied before touching the content store.

pub mod conditional;
pub mod path;
pub mod policy;
pub mod range;
