//! HTTP Handlers for Hodei Artifacts API
//!
//! This module contains all HTTP request handlers organized by domain.
//! Each handler is responsible for:
//! - Extracting request data and validating it
//! - Calling the appropriate use case
//! - Mapping results to HTTP responses
//! - Error handling and logging

pub mod health;
pub mod iam;
pub mod playground;
pub mod policies;
pub mod schemas;

// Re-export commonly used types for handlers
pub use axum::{
    extract::{Json, State},
    response::IntoResponse,
};
