//! IAM Policy Management Feature
//!
//! This module implements CRUD operations for IAM policies following
//! Vertical Slice Architecture (VSA) principles.

pub mod adapter;
pub mod dto;
pub mod error;
pub mod ports;
pub mod use_case;

#[cfg(test)]
pub mod mocks;
#[cfg(test)]
pub mod use_case_test;
