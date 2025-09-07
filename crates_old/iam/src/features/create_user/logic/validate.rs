use validator::Validate;
use crate::features::create_user::command::CreateUserCommand;
use crate::error::IamError;

// We can add more complex password validation rules here
fn is_strong_password(password: &str) -> bool {
    password.len() >= 8
}

pub fn validate_command(cmd: &CreateUserCommand) -> Result<(), IamError> {
    if cmd.username.len() < 3 {
        return Err(IamError::ValidationError("Username must be at least 3 characters long".to_string()));
    }
    if let Err(errors) = cmd.validate() {
        return Err(IamError::ValidationError(errors.to_string()));
    }
    if !is_strong_password(&cmd.password) {
        return Err(IamError::ValidationError("Password must be at least 8 characters long".to_string()));
    }
    Ok(())
}
