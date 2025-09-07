use validator::Validate;
use crate::features::update_user_attributes::command::UpdateUserAttributesCommand;
use crate::error::IamError;

pub fn validate_command(cmd: &UpdateUserAttributesCommand) -> Result<(), IamError> {
    if let Err(errors) = cmd.validate() {
        return Err(IamError::ValidationError(errors.to_string()));
    }
    // Additional validation for attributes if needed (e.g., check for specific keys or types)
    Ok(())
}
