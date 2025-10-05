//! Authorization Engine Module
//!
//! This module provides the public API for policy-based authorization.
//! All types are agnostic - Cedar is completely encapsulated as an implementation detail.
//!
//! # Architecture
//!
//! ```text
//! External Crates  →  Engine Public API  →  Translator  →  Cedar (Internal)
//!                     (Agnostic Types)                      (Hidden)
//! ```
//!
//! # Public API
//!
//! - `AuthorizationEngine` - Main engine for evaluating authorization requests
//! - `EngineRequest` - Request type (uses `&dyn HodeiEntity`)
//! - `AuthorizationDecision` - Response type (simple allow/deny)
//! - `EngineError` - Error type
//! - `PolicyDocument` - Policy representation (Cedar DSL string)
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use policies::shared::application::engine::{
//!     AuthorizationEngine, EngineRequest
//! };
//!
//! // 1. Create engine
//! let engine = AuthorizationEngine::new();
//!
//! // 2. Load policies (Cedar DSL)
//! engine.load_policies(vec![
//!     r#"permit(
//!         principal == Iam::User::"alice",
//!         action == Iam::Action::"ReadDocument",
//!         resource
//!     );"#.to_string()
//! ])?;
//!
//! // 3. Register entities
//! engine.register_entity(&user)?;
//! engine.register_entity(&document)?;
//!
//! // 4. Evaluate authorization
//! let request = EngineRequest::new(&user, "ReadDocument", &document);
//! let decision = engine.is_authorized(&request)?;
//!
//! if decision.is_allowed() {
//!     println!("Access granted!");
//! } else {
//!     println!("Access denied: {:?}", decision.reason);
//! }
//! ```
//!
//! # Key Features
//!
//! - **Agnostic API**: No Cedar types exposed
//! - **Thread-Safe**: Engine can be shared across threads with `Arc`
//! - **Type-Safe**: Uses kernel's `HodeiEntity` trait
//! - **Observable**: Integrated with `tracing` for debugging
//! - **Flexible**: Supports dynamic policy loading and entity registration
//!
//! # Design Principles
//!
//! 1. **Encapsulation**: Cedar is an internal implementation detail
//! 2. **Simplicity**: Simple boolean decisions with optional diagnostics
//! 3. **Type Safety**: Compile-time guarantees through kernel traits
//! 4. **Performance**: Efficient entity storage and policy caching
//! 5. **Debuggability**: Full tracing support for production debugging

// Module structure
mod core;
mod types;

// Re-export public types
pub use core::AuthorizationEngine;
pub use types::{
    AuthorizationDecision, Decision, EngineError, EngineRequest, PolicyDocument, SchemaConfig,
    SchemaSource,
};
