use crate::features::create_user::command::CreateUserCommand;
use super::validate::validate_command;
use serde_json::json;

#[test]
fn test_validate_command_success() {
    let cmd = CreateUserCommand {
        username: "testuser".to_string(),
        email: "test@example.com".to_string(),
        password: "password123".to_string(),
        attributes: json!({}),
    };
    assert!(validate_command(&cmd).is_ok());
}

#[test]
fn test_validate_command_short_username() {
    let cmd = CreateUserCommand {
        username: "us".to_string(),
        email: "test@example.com".to_string(),
        password: "password123".to_string(),
        attributes: json!({}),
    };
    assert!(validate_command(&cmd).is_err());
}

#[test]
fn test_validate_command_weak_password() {
    let cmd = CreateUserCommand {
        username: "testuser".to_string(),
        email: "test@example.com".to_string(),
        password: "pass".to_string(),
        attributes: json!({}),
    };
    assert!(validate_command(&cmd).is_err());
}
