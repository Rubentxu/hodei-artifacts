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

pub mod actions;
pub mod app_state;
pub mod audit;
pub mod auth;
pub mod authorization;
pub mod blob_probe;
pub mod bootstrap;
pub mod bundles;
pub mod composition_root;
pub mod config;
pub mod evidence;
pub mod evidence_dgraph;
// HSP-213: el lector del motor embebido queda tras la feature `legacy`;
// el binario productivo no la declara.
#[cfg(feature = "legacy")]
pub mod evidence_surreal;
pub mod gc;
pub mod handlers;
#[cfg(feature = "legacy")]
pub mod iam_backfill;
pub mod investigation;
pub mod migration;
pub mod oci;
pub mod openapi;
pub mod openpgp;
pub mod profile;
pub mod proxy;
pub mod public_url;
pub mod quota;
pub mod quota_dgraph;
pub mod raw;
pub mod readiness;
pub mod recovery;
pub mod repository;
#[cfg(feature = "legacy")]
pub mod repository_surreal;
pub mod resolution;
pub mod risk_facts;
pub mod router;
pub mod signed_url;
pub mod trust;

pub use app_state::AppState;
pub use bootstrap::{BootstrapConfig, bootstrap};
pub use config::AppConfig;
pub use router::build_router;
