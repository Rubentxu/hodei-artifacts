use crate::shared::domain::{CreateGroupAction, CreateUserAction, Group, Namespace, ServiceAccount, User};
/// DI Configurator for hodei-iam
/// 
/// Provides a function to configure the policies EngineBuilder with default IAM entities
use anyhow::Result;
use policies::shared::application::EngineBuilder;

/// Configure an EngineBuilder with default IAM entities
/// 
/// This function registers:
/// - Principals: User, ServiceAccount
/// - Resources: User, Group, ServiceAccount, Namespace
/// - Actions: CreateUserAction, CreateGroupAction
/// 
/// # Example
/// ```ignore
/// use policies::shared::application::di_helpers;
/// use hodei_iam::shared::application::configure_default_iam_entities;
/// 
/// # async fn example() -> anyhow::Result<()> {
/// let (engine, store) = di_helpers::build_engine_mem(configure_default_iam_entities).await?;
/// # Ok(())
/// # }
/// ```
pub fn configure_default_iam_entities(mut builder: EngineBuilder) -> Result<EngineBuilder> {
    builder
        .register_principal::<User>()?
        .register_principal::<ServiceAccount>()?
        .register_resource::<User>()?
        .register_resource::<Group>()?
        .register_resource::<ServiceAccount>()?
        .register_resource::<Namespace>()?
        .register_action::<CreateUserAction>()?
        .register_action::<CreateGroupAction>()?;
    Ok(builder)
}
