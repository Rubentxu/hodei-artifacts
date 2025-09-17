//! Mock implementations for update_policy feature testing
//!
//! This module provides mock implementations of all dependencies for unit testing.

use async_trait::async_trait;
use std::sync::Mutex;

use super::error::UpdatePolicyError;
use super::ports::{PolicyRetriever, PolicyUpdateAuditor, PolicyUpdateStorage, PolicyUpdateValidator};
use crate::domain::ids::PolicyId;
use crate::domain::policy::{Policy, PolicyVersion};
use shared::hrn::UserId;

// Mock for PolicyUpdateValidator
#[derive(Default)]
pub struct MockPolicyUpdateValidator {
    validate_policy_content_result: Mutex<Option<Result<(), UpdatePolicyError>>>,
    validate_policy_syntax_result: Mutex<Option<Result<(), UpdatePolicyError>>>,
    validate_policy_semantics_result: Mutex<Option<Result<(), UpdatePolicyError>>>,
    validate_update_allowed_result: Mutex<Option<Result<(), UpdatePolicyError>>>,
}

impl MockPolicyUpdateValidator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_validate_policy_content_result(mut self, result: Result<(), UpdatePolicyError>) -> Self {
        *self.validate_policy_content_result.lock().unwrap() = Some(result);
        self
    }

    pub fn with_validate_policy_syntax_result(mut self, result: Result<(), UpdatePolicyError>) -> Self {
        *self.validate_policy_syntax_result.lock().unwrap() = Some(result);
        self
    }

    pub fn with_validate_policy_semantics_result(mut self, result: Result<(), UpdatePolicyError>) -> Self {
        *self.validate_policy_semantics_result.lock().unwrap() = Some(result);
        self
    }

    pub fn with_validate_update_allowed_result(mut self, result: Result<(), UpdatePolicyError>) -> Self {
        *self.validate_update_allowed_result.lock().unwrap() = Some(result);
        self
    }
}

#[async_trait]
impl PolicyUpdateValidator for MockPolicyUpdateValidator {
    async fn validate_policy_content(&self, _content: &str) -> Result<(), UpdatePolicyError> {
        match &*self.validate_policy_content_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }

    async fn validate_policy_syntax(&self, _content: &str) -> Result<(), UpdatePolicyError> {
        match &*self.validate_policy_syntax_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }

    async fn validate_policy_semantics(&self, _content: &str, _policy_id: &PolicyId) -> Result<(), UpdatePolicyError> {
        match &*self.validate_policy_semantics_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }

    async fn validate_update_allowed(&self, _policy: &Policy, _user_id: &UserId) -> Result<(), UpdatePolicyError> {
        match &*self.validate_update_allowed_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }
}

// Mock for PolicyRetriever
#[derive(Default)]
pub struct MockPolicyRetriever {
    get_policy_result: Mutex<Option<Result<Option<Policy>, UpdatePolicyError>>>,
}

impl MockPolicyRetriever {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_get_policy_result(mut self, result: Result<Option<Policy>, UpdatePolicyError>) -> Self {
        *self.get_policy_result.lock().unwrap() = Some(result);
        self
    }
}

#[async_trait]
impl PolicyRetriever for MockPolicyRetriever {
    async fn get_policy(&self, _policy_id: &PolicyId) -> Result<Option<Policy>, UpdatePolicyError> {
        match &*self.get_policy_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(None), // Default None
        }
    }
}

// Mock for PolicyUpdateStorage
#[derive(Default)]
pub struct MockPolicyUpdateStorage {
    update_result: Mutex<Option<Result<(), UpdatePolicyError>>>,
    create_version_result: Mutex<Option<Result<(), UpdatePolicyError>>>,
}

impl MockPolicyUpdateStorage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_update_result(mut self, result: Result<(), UpdatePolicyError>) -> Self {
        *self.update_result.lock().unwrap() = Some(result);
        self
    }

    pub fn with_create_version_result(mut self, result: Result<(), UpdatePolicyError>) -> Self {
        *self.create_version_result.lock().unwrap() = Some(result);
        self
    }
}

#[async_trait]
impl PolicyUpdateStorage for MockPolicyUpdateStorage {
    async fn update(&self, _policy: &Policy) -> Result<(), UpdatePolicyError> {
        match &*self.update_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }

    async fn create_version(&self, _version: &PolicyVersion) -> Result<(), UpdatePolicyError> {
        match &*self.create_version_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }
}

// Mock for PolicyUpdateAuditor
#[derive(Default)]
pub struct MockPolicyUpdateAuditor {
    log_policy_update_result: Mutex<Option<Result<(), UpdatePolicyError>>>,
}

impl MockPolicyUpdateAuditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_log_policy_update_result(mut self, result: Result<(), UpdatePolicyError>) -> Self {
        *self.log_policy_update_result.lock().unwrap() = Some(result);
        self
    }
}

#[async_trait]
impl PolicyUpdateAuditor for MockPolicyUpdateAuditor {
    async fn log_policy_update(&self, _policy_id: &PolicyId, _user_id: &UserId, _changes: Vec<String>) -> Result<(), UpdatePolicyError> {
        match &*self.log_policy_update_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }
}