//! Source-aware HRN migration.
//!
//! The parser accepts the canonical v2 layout and its v1 alias (empty region).
//! Persisted data can also come from layouts whose field meaning cannot be
//! inferred from the string alone. This adapter requires the caller to declare
//! the source and, for tenant-scoped sources, the expected account. It never
//! reinterprets scope silently and never changes the stored bytes unless the
//! caller asks for a canonical write.

use thiserror::Error;

use crate::domain::hrn::Hrn;

/// The persisted layout used to read the HRN.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HrnSource {
    /// Canonical Artifacts v2 (region may be non-empty).
    ArtifactsV2,
    /// Artifacts v1: the canonical layout with an empty region.
    ArtifactsV1,
    /// `hodei-authz` 0.1, whose fourth field is a tenant. It is accepted only
    /// when the caller confirms that tenant is the expected account.
    AuthzV01,
    /// Legacy repository service (`repository`) before the canonical
    /// `artifacts` service.
    LegacyRepository,
}

/// An HRN read from a persisted layout plus its canonical representation.
///
/// `stored` is the exact input string; canonical writes use `canonical()`, and
/// the original bytes remain available for audit/dual-read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigratedHrn {
    canonical: Hrn,
    stored: String,
    source: HrnSource,
}

impl MigratedHrn {
    /// Read canonical Artifacts v1/v2.
    pub fn from_artifacts(stored: &str) -> Result<Self, HrnMigrationError> {
        let hrn = Hrn::from_string(stored).ok_or(HrnMigrationError::InvalidHrn)?;
        if hrn.partition() != "hodei" || hrn.service() != "artifacts" {
            return Err(HrnMigrationError::NotArtifacts);
        }
        let source = if hrn.region().is_empty() {
            HrnSource::ArtifactsV1
        } else {
            HrnSource::ArtifactsV2
        };
        Ok(Self {
            canonical: hrn,
            stored: stored.to_string(),
            source,
        })
    }

    /// Read a `hodei-authz` 0.1 value. The caller must confirm that its tenant
    /// is the account in scope; otherwise the value is rejected as ambiguous.
    pub fn from_authz_v01(stored: &str, expected_account: &str) -> Result<Self, HrnMigrationError> {
        let hrn = Hrn::from_string(stored).ok_or(HrnMigrationError::InvalidHrn)?;
        if hrn.partition() != "hodei" || hrn.service() != "iam" {
            return Err(HrnMigrationError::WrongSource);
        }
        if hrn.account_id() != expected_account {
            return Err(HrnMigrationError::ScopeMismatch {
                expected: expected_account.to_string(),
                actual: hrn.account_id().to_string(),
            });
        }
        Ok(Self {
            canonical: hrn,
            stored: stored.to_string(),
            source: HrnSource::AuthzV01,
        })
    }

    /// Read the legacy repository service and produce the canonical
    /// `artifacts` service. The account is preserved and checked.
    pub fn from_legacy_repository(
        stored: &str,
        expected_account: &str,
    ) -> Result<Self, HrnMigrationError> {
        let hrn = Hrn::from_string(stored).ok_or(HrnMigrationError::InvalidHrn)?;
        if hrn.partition() != "hodei"
            || hrn.service() != "repository"
            || hrn.resource_type() != "repository"
        {
            return Err(HrnMigrationError::NotLegacyRepository);
        }
        if hrn.account_id() != expected_account {
            return Err(HrnMigrationError::ScopeMismatch {
                expected: expected_account.to_string(),
                actual: hrn.account_id().to_string(),
            });
        }

        let mut canonical = hrn;
        canonical.service = "artifacts".to_string();
        Ok(Self {
            canonical,
            stored: stored.to_string(),
            source: HrnSource::LegacyRepository,
        })
    }

    /// Canonical identity for authorization and new writes.
    pub fn canonical(&self) -> &Hrn {
        &self.canonical
    }

    /// Exact bytes read from the persisted source.
    pub fn stored(&self) -> &str {
        &self.stored
    }

    /// The declared source layout used to read this value.
    pub fn source(&self) -> HrnSource {
        self.source
    }

    /// Whether the canonical representation differs from the stored bytes.
    pub fn rewrites_stored_bytes(&self) -> bool {
        self.canonical.to_string() != self.stored
    }

    /// Reconstruct the legacy repository string for a canonical `artifacts`
    /// value. This is explicit and reversible; it is not used implicitly.
    pub fn legacy_repository_string(&self) -> Option<String> {
        if self.source != HrnSource::LegacyRepository || self.canonical.service() != "artifacts" {
            return None;
        }
        let mut legacy = self.canonical.clone();
        legacy.service = "repository".to_string();
        Some(legacy.to_string())
    }
}

/// The source cannot be migrated without losing or inventing scope.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HrnMigrationError {
    #[error("invalid persisted HRN")]
    InvalidHrn,
    #[error("HRN account does not match the expected scope: expected {expected}, got {actual}")]
    ScopeMismatch { expected: String, actual: String },
    #[error("HRN is not in the legacy repository service")]
    NotLegacyRepository,
    #[error("HRN is not in the artifacts service")]
    NotArtifacts,
    #[error("HRN does not belong to the declared source")]
    WrongSource,
}
