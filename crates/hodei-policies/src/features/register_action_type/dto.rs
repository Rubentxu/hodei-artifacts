//! Data Transfer Objects for the register_action_type feature

use kernel::ActionTrait;
use serde::{Deserialize, Serialize};

/// Command for registering an action type
///
/// This command carries everything the Cedar schema builder needs to declare
/// an action: its name, an optional description and the entity types the
/// action applies to.
///
/// The action name is the leaf name of the action (`CreateUser`); the action
/// namespace is flat, which is the UID shape the evaluation engine builds at
/// request time (`Action::"CreateUser"`).
///
/// # Example
///
/// ```rust,ignore
/// use hodei_policies::features::register_action_type::dto::RegisterActionTypeCommand;
///
/// let command = RegisterActionTypeCommand::new(
///     "CreateUser".to_string(),
///     "Iam::User".to_string(),
///     "Iam::User".to_string(),
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterActionTypeCommand {
    /// The name of the action being registered
    pub action_name: String,

    /// Optional description of what this action represents
    pub description: Option<String>,

    /// The entity type the action applies to as principal
    pub applies_to_principal: String,

    /// The entity type the action applies to as resource
    pub applies_to_resource: String,
}

impl RegisterActionTypeCommand {
    /// Create a new action type registration command
    ///
    /// # Arguments
    ///
    /// * `action_name` - The name of the action to register
    /// * `applies_to_principal` - The entity type the action applies to as principal
    /// * `applies_to_resource` - The entity type the action applies to as resource
    pub fn new(
        action_name: String,
        applies_to_principal: String,
        applies_to_resource: String,
    ) -> Self {
        Self {
            action_name,
            description: None,
            applies_to_principal,
            applies_to_resource,
        }
    }

    /// Create a new action type registration command with description
    ///
    /// # Arguments
    ///
    /// * `action_name` - The name of the action to register
    /// * `applies_to_principal` - The entity type the action applies to as principal
    /// * `applies_to_resource` - The entity type the action applies to as resource
    /// * `description` - A description of what this action represents
    pub fn with_description(
        action_name: String,
        applies_to_principal: String,
        applies_to_resource: String,
        description: String,
    ) -> Self {
        Self {
            action_name,
            description: Some(description),
            applies_to_principal,
            applies_to_resource,
        }
    }

    /// Build the command from a Rust type that describes the action
    ///
    /// This is the bridge between the typed domain and the command interface:
    /// the action name and the types it applies to are read from the type
    /// itself, so the caller cannot register an action whose `appliesTo`
    /// clause disagrees with the domain.
    ///
    /// # Type Parameters
    ///
    /// * `A` - The action type to describe, must implement `ActionTrait`
    pub fn from_type<A: ActionTrait>() -> Self {
        Self {
            action_name: A::name().to_string(),
            description: None,
            applies_to_principal: A::applies_to_principal(),
            applies_to_resource: A::applies_to_resource(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::domain::ServiceName;

    struct CreateUserAction;

    impl ActionTrait for CreateUserAction {
        fn name() -> &'static str {
            "CreateUser"
        }

        fn service_name() -> ServiceName {
            ServiceName::new("iam").expect("valid service name")
        }

        fn applies_to_principal() -> String {
            "Iam::User".to_string()
        }

        fn applies_to_resource() -> String {
            "Iam::User".to_string()
        }
    }

    #[test]
    fn test_new_command() {
        let command = RegisterActionTypeCommand::new(
            "CreateUser".to_string(),
            "Iam::User".to_string(),
            "Iam::User".to_string(),
        );
        assert_eq!(command.action_name, "CreateUser");
        assert_eq!(command.applies_to_principal, "Iam::User");
        assert_eq!(command.applies_to_resource, "Iam::User");
        assert!(command.description.is_none());
    }

    #[test]
    fn test_command_with_description() {
        let command = RegisterActionTypeCommand::with_description(
            "CreateUser".to_string(),
            "Iam::User".to_string(),
            "Iam::User".to_string(),
            "Creates a new user in the system".to_string(),
        );
        assert_eq!(command.action_name, "CreateUser");
        assert_eq!(
            command.description,
            Some("Creates a new user in the system".to_string())
        );
    }

    #[test]
    fn from_type_reads_the_name_and_the_applies_to_clause_from_the_type() {
        let command = RegisterActionTypeCommand::from_type::<CreateUserAction>();

        assert_eq!(command.action_name, "CreateUser");
        assert_eq!(command.applies_to_principal, "Iam::User");
        assert_eq!(command.applies_to_resource, "Iam::User");
    }
}
