//! The prototype of the analytical projection (SPIKE-003).
//!
//! It lives in its own crate for a measured reason: LadybugDB's precompiled
//! library brings its own zstd and does not coexist with SurrealDB's in the
//! same binary. A secondary projection is a separate process, and the
//! prototype has to resemble what that would be.

#![forbid(unsafe_code)]
