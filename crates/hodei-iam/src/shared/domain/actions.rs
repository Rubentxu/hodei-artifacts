use cedar_policy::EntityTypeName;
/// Domain actions for hodei-iam
///
/// This module defines the IAM actions that can be performed
use kernel::ActionTrait;
use std::str::FromStr;

pub struct CreateUserAction;

impl ActionTrait for CreateUserAction {
    fn name() -> &'static str {
        "create_user"
    }

    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        (
            EntityTypeName::from_str("User").expect("Valid entity type"),
            EntityTypeName::from_str("User").expect("Valid entity type"),
        )
    }
}

pub struct CreateGroupAction;

impl ActionTrait for CreateGroupAction {
    fn name() -> &'static str {
        "create_group"
    }

    fn applies_to() -> (EntityTypeName, EntityTypeName) {
        (
            EntityTypeName::from_str("User").expect("Valid entity type"),
            EntityTypeName::from_str("Group").expect("Valid entity type"),
        )
    }
}
