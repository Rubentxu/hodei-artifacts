//! Hodei Artifacts API Library
//!
//! This library provides the core functionality for the Hodei Artifacts API,
//! including application state management, configuration, bootstrap logic,
//! and HTTP handlers.
//!
//! # Architecture
//!
//! The library follows Clean Architecture principles with:
//! - **Composition Root**: Bootstrap module that wires all dependencies
//! - **Application State**: Centralized state management with use cases
//! - **Handlers**: HTTP request handlers organized by domain
//! - **Configuration**: Environment-based configuration management
//!
//! # Modules
//!
//! - `app_state`: Application state containing all use cases
//! - `bootstrap`: Application initialization and dependency injection
//! - `config`: Configuration loading and validation
//! - `handlers`: HTTP request handlers for Axum
//!   - `health`: Health check endpoints
//!   - `policies`: Policy validation and evaluation
//!   - `schemas`: Schema management

pub mod app_state;
pub mod bootstrap;
pub mod config;
pub mod handlers;
pub mod openapi;

// Re-export commonly used types for external consumers
pub use app_state::AppState;
pub use bootstrap::{BootstrapConfig, bootstrap};
pub use config::Config;

// Re-export handler modules
pub use handlers::{health, policies, schemas};
pub use openapi::create_api_doc;
