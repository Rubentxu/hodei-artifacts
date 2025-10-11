use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;
use serde::{Deserialize, Serialize};

// Comando de entrada
#[derive(Deserialize, Serialize)]
pub struct ValidatePolicyCommand {
    pub content: String,
}

impl ActionTrait for ValidatePolicyCommand {
    fn name() -> &'static str {
        "ValidatePolicy"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("policies").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Policies::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Policies::Policy".to_string()
    }
}

// DTO de respuesta
#[derive(Serialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<String>,
}
