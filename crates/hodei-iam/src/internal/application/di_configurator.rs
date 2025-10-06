use crate::internal::domain::{Group, Namespace, ServiceAccount, User};
use kernel::Hrn;
/// DI Configurator for hodei-iam
/// 
/// Provides a function to configure the policies AuthorizationEngine with default IAM entities
use anyhow::Result;
use policies::shared::application::engine::AuthorizationEngine;

/// Configure an AuthorizationEngine with default IAM entities
/// 
/// This function registers:
/// - Principals: User, ServiceAccount
/// - Resources: User, Group, ServiceAccount, Namespace
/// - Actions: CreateUserAction, CreateGroupAction
/// 
/// # Example
/// ```ignore
/// use hodei_iam::shared::application::configure_default_iam_entities;
/// 
/// # async fn example() -> anyhow::Result<()> {
/// let mut engine = AuthorizationEngine::new();
/// configure_default_iam_entities(&mut engine)?;
/// # Ok(())
/// # }
/// ```
pub fn configure_default_iam_entities(engine: &mut AuthorizationEngine) -> Result<()> {
    // Create sample entities for registration
    let user_hrn = Hrn::for_entity_type::<User>("hodei".to_string(), "default".to_string(), "sample-user".to_string());
    let user = User::new(user_hrn, "Sample User".to_string(), "sample@example.com".to_string());
    
    let service_account_hrn = Hrn::for_entity_type::<ServiceAccount>("hodei".to_string(), "default".to_string(), "sample-sa".to_string());
    let service_account = ServiceAccount::new(service_account_hrn, "Sample ServiceAccount".to_string());
    
    let group_hrn = Hrn::for_entity_type::<Group>("hodei".to_string(), "default".to_string(), "sample-group".to_string());
    let group = Group::new(group_hrn, "Sample Group".to_string());
    
    let namespace_hrn = Hrn::for_entity_type::<Namespace>("hodei".to_string(), "default".to_string(), "sample-namespace".to_string());
    let namespace = Namespace::new(namespace_hrn, "Sample Namespace".to_string());

    engine
        .register_entity(&user)?;
    engine
        .register_entity(&service_account)?;
    engine
        .register_entity(&group)?;
    engine
        .register_entity(&namespace)?;
    // Note: Actions are not entities, they are handled differently in the new engine
    Ok(())
}
