//! Repository read seam.
//!
//! The active API owns a small, read-only port. The legacy `crates/repository`
//! domain is intentionally not reactivated (see
//! `docs/security/repository-baseline.md`).

use std::collections::HashMap;

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
