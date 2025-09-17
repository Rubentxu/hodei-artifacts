//! Clean mock implementations for create_policy feature testing
//!
//! This module provides clean mock implementations of all dependencies for unit testing.

use async_trait::async_trait;
use std::sync::Mutex;

use super::error::CreatePolicyError;
use super::ports::{PolicyCreationAuditor, PolicyCreationStorage, PolicyCreationValidator, PolicyExistenceChecker};
use crate::domain::ids::HodeiPolicyId;
use crate::domain::policy::{Policy, PolicyVersion};
use shared::hrn::UserId;

// Mock for PolicyCreationValidator
#[derive(Default)]
pub struct MockPolicyCreationValidator {
    validate_policy_id_result: Mutex<Option<Result<(), CreatePolicyError>>>,
    validate_policy_content_result: Mutex<Option<Result<(), CreatePolicyError>>>,
    validate_policy_syntax_result: Mutex<Option<Result<(), CreatePolicyError>>>,
    validate_policy_semantics_result: Mutex<Option<Result<(), CreatePolicyError>>>,
}

impl MockPolicyCreationValidator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_validate_policy_id_result(mut self, result: Result<(), CreatePolicyError>) -> Self {
        *self.validate_policy_id_result.lock().unwrap() = Some(result);
        self
    }

    pub fn with_validate_policy_content_result(mut self, result: Result<(), CreatePolicyError>) -> Self {
        *self.validate_policy_content_result.lock().unwrap() = Some(result);
        self
    }

    pub fn with_validate_policy_syntax_result(mut self, result: Result<(), CreatePolicyError>) -> Self {
        *self.validate_policy_syntax_result.lock().unwrap() = Some(result);
        self
    }

    pub fn with_validate_policy_semantics_result(mut self, result: Result<(), CreatePolicyError>) -> Self {
        *self.validate_policy_semantics_result.lock().unwrap() = Some(result);
        self
    }
}

#[async_trait]
impl PolicyCreationValidator for MockPolicyCreationValidator {
    async fn validate_policy_id(&self, _policy_id: &HodeiPolicyId) -> Result<(), CreatePolicyError> {
        match &*self.validate_policy_id_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }

    async fn validate_policy_content(&self, _content: &str) -> Result<(), CreatePolicyError> {
        match &*self.validate_policy_content_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }

    async fn validate_policy_syntax(&self, _content: &str) -> Result<(), CreatePolicyError> {
        match &*self.validate_policy_syntax_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }

    async fn validate_policy_semantics(&self, _content: &str, _policy_id: &HodeiPolicyId) -> Result<(), CreatePolicyError> {
        match &*self.validate_policy_semantics_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }
}

// Mock for PolicyExistenceChecker
#[derive(Default)]
pub struct MockPolicyExistenceChecker {
    exists_result: Mutex<Option<Result<bool, CreatePolicyError>>>,
}

impl MockPolicyExistenceChecker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_exists_result(mut self, result: Result<bool, CreatePolicyError>) -> Self {
        *self.exists_result.lock().unwrap() = Some(result);
        self
    }
}

#[async_trait]
impl PolicyExistenceChecker for MockPolicyExistenceChecker {
    async fn exists(&self, _policy_id: &HodeiPolicyId) -> Result<bool, CreatePolicyError> {
        match &*self.exists_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(false), // Default false (policy doesn't exist)
        }
    }
}

// Mock for PolicyCreationStorage
#[derive(Default)]
pub struct MockPolicyCreationStorage {
    save_result: Mutex<Option<Result<(), CreatePolicyError>>>,
    create_version_result: Mutex<Option<Result<(), CreatePolicyError>>>,
}

impl MockPolicyCreationStorage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_save_result(mut self, result: Result<(), CreatePolicyError>) -> Self {
        *self.save_result.lock().unwrap() = Some(result);
        self
    }

    pub fn with_create_version_result(mut self, result: Result<(), CreatePolicyError>) -> Self {
        *self.create_version_result.lock().unwrap() = Some(result);
        self
    }
}

#[async_trait]
impl PolicyCreationStorage for MockPolicyCreationStorage {
    async fn save(&self, _policy: &Policy) -> Result<(), CreatePolicyError> {
        match &*self.save_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }

    async fn create_version(&self, _version: &PolicyVersion) -> Result<(), CreatePolicyError> {
        match &*self.create_version_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }
}

// Mock for PolicyCreationAuditor
#[derive(Default)]
pub struct MockPolicyCreationAuditor {
    log_policy_creation_result: Mutex<Option<Result<(), CreatePolicyError>>>,
}

impl MockPolicyCreationAuditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_log_policy_creation_result(mut self, result: Result<(), CreatePolicyError>) -> Self {
        *self.log_policy_creation_result.lock().unwrap() = Some(result);
        self
    }
}

#[async_trait]
impl PolicyCreationAuditor for MockPolicyCreationAuditor {
    async fn log_policy_creation(&self, _policy_id: &HodeiPolicyId, _user_id: &UserId) -> Result<(), CreatePolicyError> {
        match &*self.log_policy_creation_result.lock().unwrap() {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }
}