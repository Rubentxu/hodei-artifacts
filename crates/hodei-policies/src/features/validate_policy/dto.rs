// Comando de entrada
pub struct ValidatePolicyCommand {
    pub content: String,
}

// DTO de respuesta
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<String>,
}
