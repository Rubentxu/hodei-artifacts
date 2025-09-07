use crate::application::ports::PolicyValidator;
use crate::error::IamError;
use cedar_policy::PolicySet;
use std::collections::HashSet;

pub struct CedarPolicyValidator;

impl PolicyValidator for CedarPolicyValidator {
    fn validate_policy_syntax(&self, policy_content: &str) -> Result<(), IamError> {
        match policy_content.parse::<PolicySet>() {
            Ok(_) => Ok(()),
            Err(e) => Err(IamError::ValidationError(format!(
                "Policy syntax error: {}",
                e
            ))),
        }
    }

    fn validate_policy_semantics(&self, policy_content: &str, _entities: HashSet<String>) -> Result<(), IamError> {
        // Semantic validation with Cedar typically involves linking against a schema and entities.
        // For simplicity, this example only checks if the policy can be parsed into a PolicySet.
        // A full semantic validation would require a schema and entity types.
        match policy_content.parse::<PolicySet>() {
            Ok(_) => Ok(()),
            Err(e) => Err(IamError::ValidationError(format!(
                "Policy semantic error: {}",
                e
            ))),
        }
    }
}
