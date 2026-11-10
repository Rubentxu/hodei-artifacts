//! Repository read seam.
//!
//! The active API owns a small, read-only port. The legacy `crates/repository`
//! domain is intentionally not reactivated (see
//! `docs/security/repository-baseline.md`).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use hodei_authz::{AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims};
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A repository read model. It never contains storage credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryView {
    pub hrn: String,
    pub account: String,
    pub name: String,
    pub description: Option<String>,
}

impl RepositoryView {
    pub fn new(hrn: String, account: String, name: String, description: Option<String>) -> Self {
        Self {
            hrn,
            account,
            name,
            description,
        }
    }
}

/// Read-only repository port. The account is the verified principal's account.
#[async_trait]
pub trait RepositoryReader: Send + Sync {
    async fn find_by_hrn(
        &self,
        account: &str,
        hrn: &Hrn,
    ) -> Result<Option<RepositoryView>, RepositoryReadError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepositoryReadError {
    #[error("repository storage is unavailable")]
    Unavailable,
    #[error("repository write invariants were violated")]
    InvalidRequest,
}

/// Write-only repository port for the create slice.
#[async_trait]
pub trait RepositoryWriter: Send + Sync {
    async fn create(
        &self,
        account: &str,
        hrn: &Hrn,
        view: RepositoryView,
    ) -> Result<RepositoryView, RepositoryWriteError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepositoryWriteError {
    #[error("repository already exists")]
    AlreadyExists,
    #[error("repository storage is unavailable")]
    Unavailable,
    #[error("repository write invariants were violated")]
    InvalidRequest,
}

/// Deterministic in-memory reader for the vertical slice and tests.
#[derive(Debug, Clone, Default)]
pub struct InMemoryRepositoryReader {
    rows: HashMap<String, RepositoryView>,
}

impl InMemoryRepositoryReader {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, hrn: Hrn, view: RepositoryView) {
        self.rows.insert(hrn.to_string(), view);
    }

    pub fn with(mut self, hrn: Hrn, view: RepositoryView) -> Self {
        self.insert(hrn, view);
        self
    }
}

#[async_trait]
impl RepositoryReader for InMemoryRepositoryReader {
    async fn find_by_hrn(
        &self,
        account: &str,
        hrn: &Hrn,
    ) -> Result<Option<RepositoryView>, RepositoryReadError> {
        Ok(self
            .rows
            .get(&hrn.to_string())
            .filter(|view| view.account == account)
            .cloned())
    }
}

/// Shared in-memory store implementing both ports with an atomic uniqueness
/// check under one lock (no check-then-insert TOCTOU).
#[derive(Debug, Clone, Default)]
pub struct InMemoryRepositoryStore {
    rows: Arc<RwLock<HashMap<String, RepositoryView>>>,
}

impl InMemoryRepositoryStore {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn insert(&self, hrn: Hrn, view: RepositoryView) {
        self.rows
            .write()
            .expect("repository store lock")
            .insert(hrn.to_string(), view);
    }

    pub fn with(self, hrn: Hrn, view: RepositoryView) -> Self {
        self.insert(hrn, view);
        self
    }

    pub fn len(&self) -> usize {
        self.rows.read().expect("repository store lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[async_trait]
impl RepositoryReader for InMemoryRepositoryStore {
    async fn find_by_hrn(
        &self,
        account: &str,
        hrn: &Hrn,
    ) -> Result<Option<RepositoryView>, RepositoryReadError> {
        Ok(self
            .rows
            .read()
            .expect("repository store lock")
            .get(&hrn.to_string())
            .filter(|view| view.account == account)
            .cloned())
    }
}

#[async_trait]
impl RepositoryWriter for InMemoryRepositoryStore {
    async fn create(
        &self,
        account: &str,
        hrn: &Hrn,
        view: RepositoryView,
    ) -> Result<RepositoryView, RepositoryWriteError> {
        let key = hrn.to_string();
        let mut rows = self.rows.write().expect("repository store lock");
        if rows.contains_key(&key) {
            return Err(RepositoryWriteError::AlreadyExists);
        }
        if view.account != account || view.hrn != key {
            return Err(RepositoryWriteError::InvalidRequest);
        }
        rows.insert(key, view.clone());
        Ok(view)
    }
}

/// Fail-closed authenticator used until a real credential adapter is configured.
///
/// The repository route is still mounted, but every request without an
/// injected authenticator is `401`; it is never opened by
/// `admin_api_enabled`.
pub struct DenyAllAuthenticator;

#[async_trait]
impl PrincipalAuthenticator for DenyAllAuthenticator {
    async fn authenticate(
        &self,
        _credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        Err(AuthenticationError::InvalidCredentials)
    }
}
