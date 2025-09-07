use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::features::update_user_attributes::command::UpdateUserAttributesCommand;
use crate::features::update_user_attributes::logic::use_case::UpdateUserAttributesUseCase;

// This is the new handle function that will be used by the IamApi
pub async fn handle_update_user_attributes(
    user_repository: &dyn UserRepository,
    command: UpdateUserAttributesCommand,
) -> Result<(), IamError> {
    let use_case = UpdateUserAttributesUseCase::new(user_repository);
    use_case.execute(command).await
}

