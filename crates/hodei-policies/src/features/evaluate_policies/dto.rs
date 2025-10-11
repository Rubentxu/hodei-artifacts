use kernel::domain::policy::HodeiPolicySet;
use std::collections::HashMap;
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;

/// Mode for policy evaluation regarding schema usage
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvaluationMode {
    /// Strict mode: requires schema to be loaded, fails if not found
    Strict,
    /// Best effort: tries to load schema but falls back to no-schema evaluation if not found
    BestEffortNoSchema,
    /// Explicit no schema: evaluates without loading any schema
    NoSchema,
}

impl Default for EvaluationMode {
    fn default() -> Self {
        Self::BestEffortNoSchema
    }
}

/// Command for evaluating authorization policies
pub struct EvaluatePoliciesCommand<'a> {
    /// The authorization request to evaluate
    pub request: AuthorizationRequest<'a>,

    /// The policy set to evaluate against
    pub policies: &'a HodeiPolicySet,

    /// Entities involved in the evaluation
    pub entities: &'a [&'a dyn kernel::HodeiEntity],

    /// Optional specific schema version to use for evaluation
    /// If None and mode allows it, will try to load the latest schema
    pub schema_version: Option<String>,

    /// Evaluation mode regarding schema usage
    pub evaluation_mode: EvaluationMode,
}

impl ActionTrait for EvaluatePoliciesCommand<'_> {
    fn name() -> &'static str {
        "EvaluatePolicies"
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

impl<'a> EvaluatePoliciesCommand<'a> {
    /// Create a new evaluation command with default settings (BestEffortNoSchema mode)
    pub fn new(
        request: AuthorizationRequest<'a>,
        policies: &'a HodeiPolicySet,
        entities: &'a [&'a dyn kernel::HodeiEntity],
    ) -> Self {
        Self {
            request,
            policies,
            entities,
            schema_version: None,
            evaluation_mode: EvaluationMode::default(),
        }
    }

    /// Set a specific schema version to use
    pub fn with_schema_version(mut self, version: impl Into<String>) -> Self {
        self.schema_version = Some(version.into());
        self
    }

    /// Set the evaluation mode
    pub fn with_evaluation_mode(mut self, mode: EvaluationMode) -> Self {
        self.evaluation_mode = mode;
        self
    }

    /// Use strict schema mode (requires schema)
    pub fn strict_schema(mut self) -> Self {
        self.evaluation_mode = EvaluationMode::Strict;
        self
    }

    /// Use no schema mode (explicit no schema loading)
    pub fn no_schema(mut self) -> Self {
        self.evaluation_mode = EvaluationMode::NoSchema;
        self
    }
}

/// Authorization request containing principal, action, resource, and context
pub struct AuthorizationRequest<'a> {
    /// The principal (user/entity) making the request
    pub principal: &'a dyn kernel::HodeiEntity,

    /// The action being requested
    pub action: &'a str,

    /// The resource being accessed
    pub resource: &'a dyn kernel::HodeiEntity,

    /// Optional context for the evaluation
    pub context: Option<HashMap<String, serde_json::Value>>,
}

impl<'a> AuthorizationRequest<'a> {
    /// Create a new authorization request
    pub fn new(
        principal: &'a dyn kernel::HodeiEntity,
        action: &'a str,
        resource: &'a dyn kernel::HodeiEntity,
    ) -> Self {
        Self {
            principal,
            action,
            resource,
            context: None,
        }
    }

    /// Add context to the request
    pub fn with_context(mut self, context: HashMap<String, serde_json::Value>) -> Self {
        self.context = Some(context);
        self
    }
}

/// Decision result from policy evaluation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Access is allowed
    Allow,
    /// Access is denied
    Deny,
}

impl Default for Decision {
    fn default() -> Self {
        Self::Deny
    }
}

/// Diagnostic information about the evaluation
#[derive(Debug, Clone)]
pub struct EvaluationDiagnostic {
    /// Type of diagnostic message
    pub level: DiagnosticLevel,
    /// Diagnostic message
    pub message: String,
    /// Optional policy ID related to this diagnostic
    pub policy_id: Option<String>,
}

/// Level of diagnostic information
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticLevel {
    /// Informational message
    Info,
    /// Warning message
    Warning,
    /// Error message
    Error,
}

/// Result of policy evaluation
#[derive(Debug)]
pub struct EvaluationDecision {
    /// The final decision (Allow or Deny)
    pub decision: Decision,

    /// IDs of policies that determined the decision
    pub determining_policies: Vec<String>,

    /// Reasons and explanations from Cedar
    pub reasons: Vec<String>,

    /// Schema version used during evaluation (if any)
    pub used_schema_version: Option<String>,

    /// IDs of all policies evaluated
    pub policy_ids_evaluated: Vec<String>,

    /// Diagnostic information about the evaluation
    pub diagnostics: Vec<EvaluationDiagnostic>,
}

impl EvaluationDecision {
    /// Create a new evaluation decision with minimal information
    pub fn new(decision: Decision) -> Self {
        Self {
            decision,
            determining_policies: vec![],
            reasons: vec![],
            used_schema_version: None,
            policy_ids_evaluated: vec![],
            diagnostics: vec![],
        }
    }

    /// Add a diagnostic message
    pub fn add_diagnostic(mut self, level: DiagnosticLevel, message: impl Into<String>) -> Self {
        self.diagnostics.push(EvaluationDiagnostic {
            level,
            message: message.into(),
            policy_id: None,
        });
        self
    }

    /// Set the schema version that was used
    pub fn with_schema_version(mut self, version: Option<String>) -> Self {
        self.used_schema_version = version;
        self
    }
}
