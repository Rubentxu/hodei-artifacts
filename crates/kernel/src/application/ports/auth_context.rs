//! Authentication context abstraction for the shared kernel
//!
//! This module provides traits for obtaining authentication and authorization
//! context information across bounded contexts. Implementations are provided
//! by the application layer (e.g., from HTTP headers, JWT tokens, etc.).

use crate::domain::Hrn;
use async_trait::async_trait;
use thiserror::Error;

/// Errors that can occur when accessing authentication context
#[derive(Debug, Error)]
pub enum AuthContextError {
    /// No authenticated user/principal in the current context
    #[error("No authenticated user in current context")]
    NoCurrentUser,

    /// The current user lacks the required permissions
    #[error("Insufficient permissions: {0}")]
    InsufficientPermissions(String),

    /// Invalid or expired authentication token
    #[error("Invalid authentication token: {0}")]
    InvalidToken(String),

    /// Invalid tenant/organization context
    #[error("Invalid tenant context: {0}")]
    InvalidTenant(String),

    /// Generic authentication/authorization error
    #[error("Authentication error: {0}")]
    Other(String),
}

/// Provides authentication and authorization context for the current operation.
///
/// This trait abstracts the source of authentication information (e.g., JWT token,
/// session, API key) and provides a consistent interface for use cases to obtain
/// the current principal, tenant, and permission information.
///
/// # Examples
///
/// ```rust,ignore
/// use kernel::application::ports::AuthContextProvider;
///
/// async fn my_use_case(auth: &dyn AuthContextProvider) -> Result<(), AuthContextError> {
///     // Get the current authenticated user
///     let user_hrn = auth.current_principal_hrn().await?;
///
///     // Check if user has specific permission
///     if auth.has_permission("iam:CreateUser").await? {
///         // Proceed with operation
///     }
///
///     // Get tenant context if multi-tenant
///     if let Some(tenant) = auth.tenant_hrn().await? {
///         // Use tenant for scoping
///     }
///
///     Ok(())
/// }
/// ```
#[async_trait]
pub trait AuthContextProvider: Send + Sync {
    /// Returns the HRN of the current authenticated principal (user, service account, etc.)
    ///
    /// # Errors
    ///
    /// Returns `AuthContextError::NoCurrentUser` if no principal is authenticated.
    async fn current_principal_hrn(&self) -> Result<Hrn, AuthContextError>;

    /// Checks if the current principal has the specified permission.
    ///
    /// The permission format is typically "service:Action" (e.g., "iam:CreateUser").
    ///
    /// # Errors
    ///
    /// Returns `AuthContextError::NoCurrentUser` if no principal is authenticated.
    async fn has_permission(&self, permission: &str) -> Result<bool, AuthContextError>;

    /// Returns the HRN of the current tenant/organization context, if applicable.
    ///
    /// In multi-tenant systems, this identifies which tenant the current operation
    /// is being performed for. Returns `None` for single-tenant systems or when
    /// operating in a global context.
    ///
    /// # Errors
    ///
    /// Returns `AuthContextError::InvalidTenant` if tenant context is required but invalid.
    async fn tenant_hrn(&self) -> Result<Option<Hrn>, AuthContextError>;

    /// Returns optional session metadata (e.g., IP address, user agent, session ID)
    ///
    /// This can be used for audit logging, rate limiting, or security checks.
    async fn session_metadata(&self) -> Result<SessionMetadata, AuthContextError> {
        // Default implementation returns empty metadata
        Ok(SessionMetadata::default())
    }
}

/// Metadata about the current authentication session
#[derive(Debug, Clone, Default)]
pub struct SessionMetadata {
    /// IP address of the client
    pub ip_address: Option<String>,

    /// User agent string
    pub user_agent: Option<String>,

    /// Unique session identifier
    pub session_id: Option<String>,

    /// Timestamp when the session was established
    pub established_at: Option<chrono::DateTime<chrono::Utc>>,

    /// Additional custom metadata
    pub custom_fields: std::collections::HashMap<String, String>,
}
