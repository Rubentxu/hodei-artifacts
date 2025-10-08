//! Adapters for Playground Evaluate Feature
//!
//! This module contains concrete implementations of the playground_evaluate
//! port traits, integrating with Cedar's authorization engine and other
//! hodei-policies features.
//!
//! # Available Adapters
//!
//! - **SchemaLoaderAdapter**: Loads schemas from inline JSON or storage
//! - **PolicyValidatorAdapter**: Validates policies against schemas
//! - **PolicyEvaluatorAdapter**: Evaluates authorization requests
//! - **ContextConverterAdapter**: Converts context attributes to Cedar format
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use hodei_policies::features::playground_evaluate::adapters::{
//!     SchemaLoaderAdapter,
//!     PolicyValidatorAdapter,
//!     PolicyEvaluatorAdapter,
//!     ContextConverterAdapter,
//! };
//! use hodei_policies::features::playground_evaluate::PlaygroundEvaluateUseCaseFactory;
//!
//! // Create adapters
//! let schema_loader = Arc::new(SchemaLoaderAdapter::new_inline_only());
//! let policy_validator = Arc::new(PolicyValidatorAdapter::new());
//! let policy_evaluator = Arc::new(PolicyEvaluatorAdapter::new());
//! let context_converter = Arc::new(ContextConverterAdapter::new());
//!
//! // Build use case with real adapters
//! let use_case = PlaygroundEvaluateUseCaseFactory::build(
//!     schema_loader,
//!     policy_validator,
//!     policy_evaluator,
//!     context_converter,
//! );
//! ```

pub mod context_converter;
pub mod policy_evaluator;
pub mod policy_validator;
pub mod schema_loader;

// Re-export for convenience
pub use context_converter::ContextConverterAdapter;
pub use policy_evaluator::PolicyEvaluatorAdapter;
pub use policy_validator::PolicyValidatorAdapter;
pub use schema_loader::SchemaLoaderAdapter;
