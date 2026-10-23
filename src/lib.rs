//! Hodei Artifacts API — library entry point.
//!
//! The binary in `src/main.rs` is a thin shell around this library: it loads
//! configuration, initializes logging, bootstraps the composition root and
//! serves the router.
//!
//! Exposing the application as a library is what makes the HTTP contract
//! testable in-process (see `tests/http_contract.rs`). Before this target
//! existed, the root package shipped as a binary only, so `tests/e2e/` and
//! `tests/integration/` could never be compiled: they referenced
//! `hodei_artifacts_api::...` paths that did not exist.
//!
//! Layering: this crate owns *effects* only (HTTP, configuration, wiring).
//! Business logic lives in the bounded contexts (`kernel`, `hodei-iam`,
//! `hodei-policies`), which are consumed through their port traits.

pub mod app_state;
pub mod bootstrap;
pub mod composition_root;
pub mod config;
pub mod handlers;
pub mod openapi;
pub mod router;

pub use app_state::AppState;
pub use bootstrap::{BootstrapConfig, bootstrap};
pub use config::AppConfig;
pub use router::build_router;
