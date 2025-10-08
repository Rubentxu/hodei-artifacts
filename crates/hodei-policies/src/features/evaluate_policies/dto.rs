use kernel::domain::policy::HodeiPolicySet;
use std::collections::HashMap;

// Comando de entrada
pub struct EvaluatePoliciesCommand<'a> {
    pub request: AuthorizationRequest<'a>,
    pub policies: &'a HodeiPolicySet,
    pub entities: &'a [&'a dyn kernel::HodeiEntity],
}

pub struct AuthorizationRequest<'a> {
    pub principal: &'a dyn kernel::HodeiEntity,
    pub action: &'a str,
    pub resource: &'a dyn kernel::HodeiEntity,
    pub context: Option<HashMap<String, serde_json::Value>>, // Contexto simple para la evaluación
}

// DTO de respuesta
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
}

#[derive(Debug)]
pub struct EvaluationDecision {
    pub decision: Decision,
    pub determining_policies: Vec<String>, // IDs de las políticas que llevaron a la decisión
    pub reasons: Vec<String>,              // Explicaciones de Cedar
}
