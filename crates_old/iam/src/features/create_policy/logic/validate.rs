use validator::Validate;
use crate::features::create_policy::command::CreatePolicyCommand;
use crate::error::IamError;
use crate::application::ports::PolicyValidator;
use std::collections::HashSet;

pub fn validate_command(cmd: &CreatePolicyCommand, policy_validator: &dyn PolicyValidator) -> Result<(), IamError> {
    if let Err(errors) = cmd.validate() {
        return Err(IamError::ValidationError(errors.to_string()));
    }

    // Validate policy syntax
    policy_validator.validate_policy_syntax(&cmd.content)?;

    // Validate policy semantics (with empty entities for now)
    // TODO: Pass actual entities for more robust semantic validation
    policy_validator.validate_policy_semantics(&cmd.content, HashSet::new())?;

    Ok(())
}


