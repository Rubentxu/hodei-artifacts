//! Integration Tests Module
//!
//! This module contains integration tests for all IAM policy handlers.
//! Each handler has comprehensive test coverage including:
//! - Success scenarios (happy path)
//! - Validation errors
//! - Edge cases
//! - Concurrency scenarios
//! - Performance tests
//!
//! All tests use real SurrealDB instances via testcontainers for
//! complete integration testing.

#[path = "../common/mod.rs"]
mod common;

mod test_create_policy;
mod test_get_policy;
mod test_list_policies;
mod test_update_policy;
mod test_delete_policy;
mod test_policy_lifecycle;
mod test_concurrency;
