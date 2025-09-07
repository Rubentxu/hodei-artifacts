use crate::features::create_policy::command::CreatePolicyCommand;
use super::validate::validate_command;
use crate::application::ports::PolicyValidator as MockPolicyValidator;
use crate::error::IamError;
use std::collections::HashSet;

struct MockValidator;

impl MockPolicyValidator for MockValidator {
    fn validate_policy_syntax(&self, _policy_content: &str) -> Result<(), IamError> {
        Ok(())
    }

    fn validate_policy_semantics(&self, _policy_content: &str, _entities: HashSet<String>) -> Result<(), IamError> {
        Ok(())
    }
}

#[test]
fn test_validate_command_success() {
    let cmd = CreatePolicyCommand {
        name: "test_policy".to_string(),
        description: None,
        content: "permit(principal, action, resource);".to_string(),
    };
    let validator = MockValidator;
    assert!(validate_command(&cmd, &validator).is_ok());
}

#[test]
fn test_validate_command_empty_name() {
    let cmd = CreatePolicyCommand {
        name: "".to_string(),
        description: None,
        content: "permit(principal, action, resource);".to_string(),
    };
    let validator = MockValidator;
    assert!(validate_command(&cmd, &validator).is_err());
}

#[test]
fn test_validate_command_empty_content() {
    let cmd = CreatePolicyCommand {
        name: "test_policy".to_string(),
        description: None,
        content: "".to_string(),
    };
    let validator = MockValidator;
    assert!(validate_command(&cmd, &validator).is_err());
}
