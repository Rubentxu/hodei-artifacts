//! Catalog errors.
//!
//! The distinction that matters most here is between **Missing**, **Denied** and
//! **Corrupt**, because all three look alike and are not the same thing:
//!
//! - **Missing**: there is nothing there. The request is valid and the resource
//!   does not exist.
//! - **Denied**: there is something there and this principal cannot see it. The
//!   request is valid and the permission is not.
//! - **Corrupt**: there is something there and it cannot be read properly. The
//!   request is valid and the data is broken.
//!
//! Confusing them is worse than not answering: a `Missing` where there was a
//! `Denied` makes the client retry; a `Corrupt` where there was a
//! `Missing` makes the client delete something that was not theirs.

use thiserror::Error;

use crate::model::RepositoryMode;

/// What can fail in the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CatalogError {
    /// What was requested does not exist.
    #[error("there is no {kind} for {key}")]
    Missing {
        /// What was requested.
        kind: &'static str,
        /// With which coordinates it was requested.
        key: String,
    },

    /// The principal does not have permission.
    #[error("the principal is not allowed to {action} the {kind} {key}: {reason}")]
    Denied {
        /// What was attempted.
        action: &'static str,
        /// On what.
        kind: &'static str,
        /// With which coordinates.
        key: String,
        /// Why not.
        reason: String,
    },

    /// The record exists but cannot be read properly.
    #[error("the {kind} {key} is corrupt: {detail}")]
    Corrupt {
        /// What was requested.
        kind: &'static str,
        /// With which coordinates.
        key: String,
        /// What is broken.
        detail: String,
    },

    /// There is already something there with different content.
    ///
    /// This is a conflict and not a failure: the database refuses to
    /// overwrite, and the catalog does not invent a rule to do so.
    #[error("the {kind} {key} already exists with different content: {detail}")]
    Conflict {
        /// What was attempted to be written.
        kind: &'static str,
        /// With which coordinates.
        key: String,
        /// How it differs from what is there.
        detail: String,
    },

    /// The condition the client required is not met.
    ///
    /// This is not a conflict: a conflict says there is something different from
    /// what is being published, and the precondition says that **what is there** is not
    /// what the client expected. The difference matters because the client
    /// treats them differently: a conflict is resolved by changing what is
    /// published, and a precondition by re-reading what is there.
    #[error("the precondition for the {kind} {key} does not hold: {detail}")]
    PreconditionFailed {
        /// On what.
        kind: &'static str,
        /// With which coordinates.
        key: String,
        /// How it differs from what the client expected.
        detail: String,
    },

    /// The repository mode is not served by this release.
    #[error("the repository mode {mode} is not served by this release")]
    ModeNotImplemented {
        /// The requested mode.
        mode: RepositoryMode,
    },

    /// The request makes no sense.
    #[error("the request is not valid: {detail}")]
    InvalidRequest {
        /// Why not.
        detail: String,
    },

    /// The store has a schema that this build does not understand.
    ///
    /// This is not a storage failure: it is a store that was written
    /// with a newer version than this one. Declaring the schema on top does not
    /// fix it (existing definitions are not redone), so
    /// the only honest response is to stop and say so.
    #[error(
        "the catalog storage has the schema version {found} and this build understands {expected}"
    )]
    SchemaMismatch {
        /// The version found in the store.
        found: i64,
        /// The version this build understands.
        expected: i64,
    },

    /// Storage could not respond.
    #[error("the catalog storage is unavailable: {detail}")]
    Unavailable {
        /// What storage said.
        detail: String,
    },
}

impl CatalogError {
    /// The "there is nothing there" error.
    pub fn missing(kind: &'static str, key: impl Into<String>) -> Self {
        CatalogError::Missing {
            kind,
            key: key.into(),
        }
    }

    /// The "there is something there and it is broken" error.
    pub fn corrupt(kind: &'static str, key: impl Into<String>, detail: impl Into<String>) -> Self {
        CatalogError::Corrupt {
            kind,
            key: key.into(),
            detail: detail.into(),
        }
    }

    /// The "there is already something there with different content" error.
    pub fn conflict(kind: &'static str, key: impl Into<String>, detail: impl Into<String>) -> Self {
        CatalogError::Conflict {
            kind,
            key: key.into(),
            detail: detail.into(),
        }
    }

    /// The "the request makes no sense" error.
    pub fn invalid(detail: impl Into<String>) -> Self {
        CatalogError::InvalidRequest {
            detail: detail.into(),
        }
    }

    /// The "the condition the client required is not met" error.
    pub fn precondition_failed(
        kind: &'static str,
        key: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        CatalogError::PreconditionFailed {
            kind,
            key: key.into(),
            detail: detail.into(),
        }
    }

    /// The "storage could not respond" error.
    pub fn unavailable(detail: impl Into<String>) -> Self {
        CatalogError::Unavailable {
            detail: detail.into(),
        }
    }
}
