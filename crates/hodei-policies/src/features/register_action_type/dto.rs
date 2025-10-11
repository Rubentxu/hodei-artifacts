//! Data Transfer Objects for the register_action_type feature

use serde::{Deserialize, Serialize};
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;

/// Command for registering an action type
///
/// This command encapsulates the data needed to register an action type
/// in the Cedar schema builder. It serves as the input contract for the
/// use case when using the command-based interface.
///
/// Note: In practice, direct generic registration via `use_case.register::<A>()`
/// is preferred for type safety, but this command exists to satisfy the
/// port trait interface for architectural consistency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterActionTypeCommand {
    /// The name of the action type being registered
    pub action_name: String,

    /// Optional description of what this action represents
    pub description: Option<String>,
}

impl ActionTrait for RegisterActionTypeCommand {
    fn name() -> &'static str {
        "RegisterActionType"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("policies").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Policies::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Policies::ActionType".to_string()
    }
}

impl RegisterActionTypeCommand {
    /// Create a new action type registration command
    ///
    /// # Arguments
    ///
    /// * `action_name` - The name of the action to register
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hodei_policies::features::register_action_type::dto::RegisterActionTypeCommand;
    ///
    /// let command = RegisterActionTypeCommand::new("CreateUser".to_string());
    /// ```
    pub fn new(action_name: String) -> Self {
        Self {
            action_name,
            description: None,
        }
    }

    /// Create a new action type registration command with description
    ///
    /// # Arguments
    ///
    /// * `action_name` - The name of the action to register
    /// * `description` - A description of what this action represents
    pub fn with_description(action_name: String, description: String) -> Self {
        Self {
            action_name,
            description: Some(description),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_command() {
        let command = RegisterActionTypeCommand::new("CreateUser".to_string());
        assert_eq!(command.action_name, "CreateUser");
        assert!(command.description.is_none());
    }

    #[test]
    fn test_command_with_description() {
        let command = RegisterActionTypeCommand::with_description(
            "CreateUser".to_string(),
            "Creates a new user in the system".to_string(),
        );
        assert_eq!(command.action_name, "CreateUser");
        assert_eq!(
            command.description,
            Some("Creates a new user in the system".to_string())
        );
    }
}
