//! Policy evaluation feature
//! 
//! This module provides comprehensive policy evaluation capabilities using the Cedar policy engine.
//! It implements segregated interfaces following VSA architecture principles.

pub mod use_case;
pub mod dto;
pub mod error;
pub mod ports;
pub mod adapter;
pub mod event_handler;
pub mod di;
pub mod mocks;
mod use_case_test;

pub use adapter::PolicyEvaluationAdapter;
pub use dto::*;
pub use error::EvaluatePolicyError;
pub use ports::*;
// Re-export main types for easier access
pub use use_case::EvaluatePolicyUseCase;
