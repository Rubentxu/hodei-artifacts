//! Ports (public contracts) for the `register_iam_schema` feature.
//!
//! This feature orchestrates the registration of IAM-specific entity and action
//! types in the policies engine and then triggers schema building. Other
//! bounded contexts (or the application composition root) should depend on
//! this trait rather than the concrete use case to preserve the Dependency
//! Inversion Principle and enable test doubles.
//!
//! Architectural Notes
//! -------------------
//! - This trait is intentionally minimal (ISP) and only exposes the single
//!   orchestration operation required by callers.
//! - It returns domain-specific DTOs and errors defined within this vertical
//!   slice (`dto.rs` / `error.rs`), avoiding leakage of internal dependencies.
//! - The concrete implementation lives in `use_case.rs` and implements this
//!   trait via `async_trait`.
//!
//! Example (composition root pseudo-code)
//! --------------------------------------
//! ```ignore
//! use hodei_iam::features::register_iam_schema::{
//!     RegisterIamSchemaCommand, RegisterIamSchemaUseCase
//! };
//!
//! // Build dependencies (other use cases from hodei-policies)
//! let uc = RegisterIamSchemaUseCase::new(entity_uc, action_uc, build_uc);
//! let result = uc.register(RegisterIamSchemaCommand::new().with_validation(true)).await?;
//! println!("Schema version: {}", result.schema_version);
//! ```

use async_trait::async_trait;

use super::dto::{RegisterIamSchemaCommand, RegisterIamSchemaResult};
use super::error::RegisterIamSchemaError;

/// Port trait for registering the IAM schema.
///
/// Implementations perform:
/// 1. Registration of all IAM entity types
/// 2. Registration of all IAM action types
/// 3. Schema build + (optional) validation + persistence
#[async_trait]
pub trait RegisterIamSchemaPort: Send + Sync {
    /// Orchestrates the full IAM schema registration workflow.
    ///
    /// # Errors
    /// Returns `RegisterIamSchemaError` if any sub-step fails (entity/action
    /// registration, schema build, validation, or persistence).
    async fn register(
        &self,
        command: RegisterIamSchemaCommand,
    ) -> Result<RegisterIamSchemaResult, RegisterIamSchemaError>;
}
