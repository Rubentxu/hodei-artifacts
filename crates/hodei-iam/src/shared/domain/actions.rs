//! Domain actions for hodei-iam
//!
//! This module defines the IAM actions that can be performed.
//! All actions implement the agnostic ActionTrait.

use kernel::{ActionTrait, ServiceName};

// ============================================================================
// CreateUser Action
// ============================================================================

/// Action for creating a new user
pub struct CreateUserAction;

impl ActionTrait for CreateUserAction {
    fn name() -> &'static str {
        "CreateUser"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::User".to_string()
    }
}

// ============================================================================
// CreateGroup Action
// ============================================================================

/// Action for creating a new group
pub struct CreateGroupAction;

impl ActionTrait for CreateGroupAction {
    fn name() -> &'static str {
        "CreateGroup"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Group".to_string()
    }
}

// ============================================================================
// DeleteUser Action
// ============================================================================

/// Action for deleting a user
pub struct DeleteUserAction;

impl ActionTrait for DeleteUserAction {
    fn name() -> &'static str {
        "DeleteUser"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::User".to_string()
    }
}

// ============================================================================
// DeleteGroup Action
// ============================================================================

/// Action for deleting a group
pub struct DeleteGroupAction;

impl ActionTrait for DeleteGroupAction {
    fn name() -> &'static str {
        "DeleteGroup"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Group".to_string()
    }
}

// ============================================================================
// AddUserToGroup Action
// ============================================================================

/// Action for adding a user to a group
pub struct AddUserToGroupAction;

impl ActionTrait for AddUserToGroupAction {
    fn name() -> &'static str {
        "AddUserToGroup"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Group".to_string()
    }
}

// ============================================================================
// RemoveUserFromGroup Action
// ============================================================================

/// Action for removing a user from a group
pub struct RemoveUserFromGroupAction;

impl ActionTrait for RemoveUserFromGroupAction {
    fn name() -> &'static str {
        "RemoveUserFromGroup"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Group".to_string()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_user_action() {
        assert_eq!(CreateUserAction::name(), "CreateUser");
        assert_eq!(CreateUserAction::service_name().as_str(), "iam");
        assert_eq!(
            CreateUserAction::action_name(),
            "Iam::Action::\"CreateUser\""
        );
        assert_eq!(CreateUserAction::applies_to_principal(), "Iam::User");
        assert_eq!(CreateUserAction::applies_to_resource(), "Iam::User");
    }

    #[test]
    fn test_create_group_action() {
        assert_eq!(CreateGroupAction::name(), "CreateGroup");
        assert_eq!(CreateGroupAction::service_name().as_str(), "iam");
        assert_eq!(
            CreateGroupAction::action_name(),
            "Iam::Action::\"CreateGroup\""
        );
        assert_eq!(CreateGroupAction::applies_to_principal(), "Iam::User");
        assert_eq!(CreateGroupAction::applies_to_resource(), "Iam::Group");
    }

    #[test]
    fn test_delete_user_action() {
        assert_eq!(DeleteUserAction::name(), "DeleteUser");
        assert_eq!(DeleteUserAction::applies_to_principal(), "Iam::User");
        assert_eq!(DeleteUserAction::applies_to_resource(), "Iam::User");
    }

    #[test]
    fn test_add_user_to_group_action() {
        assert_eq!(AddUserToGroupAction::name(), "AddUserToGroup");
        assert_eq!(AddUserToGroupAction::applies_to_resource(), "Iam::Group");
    }
}
