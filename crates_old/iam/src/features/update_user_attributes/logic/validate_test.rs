use crate::features::update_user_attributes::command::UpdateUserAttributesCommand;
use super::validate::validate_command;
use serde_json::json;
use shared::UserId;

#[test]
fn test_validate_command_success() {
    let cmd = UpdateUserAttributesCommand {
        user_id: UserId::new(),
        attributes: json!("{\"department\": \"engineering\"}"),
    };
    assert!(validate_command(&cmd).is_ok());
}

#[test]
fn test_validate_command_empty_attributes() {
    let cmd = UpdateUserAttributesCommand {
        user_id: UserId::new(),
        attributes: json!({}),
    };
    assert!(validate_command(&cmd).is_ok()); // Empty attributes are valid
}

