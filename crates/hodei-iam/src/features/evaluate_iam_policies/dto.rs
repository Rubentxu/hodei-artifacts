//! Data Transfer Objects for the evaluate_iam_policies feature
//!
//! The command is what the consumer asks; the result is what the consumer
//! learns. Both are expressed in terms the consumer can read: the decision,
//! the reason, and the policies that were actually consulted.

use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// Command to evaluate the IAM policies of a principal
///
/// The three fields are the whole question: *who* asks, *what* they want to
/// do, and *on which* resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluateIamPoliciesCommand {
    /// The principal the decision is about.
    pub principal_hrn: Hrn,

    /// The verb being requested.
    pub action_name: String,

    /// The resource the verb is requested on.
    pub resource_hrn: Hrn,
}

/// The outcome of an IAM policy evaluation
///
/// `determining_policies` names the policies that determined the decision and
/// `policy_ids_evaluated` names the policies that were consulted at all.
/// The difference matters: a policy that was consulted and did not determine
/// anything is not evidence that it was applied, and a policy that determined
/// the decision is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluateIamPoliciesResult {
    /// Whether the request is allowed.
    pub decision: bool,

    /// Why the decision was reached.
    pub reason: String,

    /// The ids of the policies that determined the decision.
    pub determining_policies: Vec<String>,

    /// The ids of the policies that were consulted.
    pub policy_ids_evaluated: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_round_trips() {
        let command = EvaluateIamPoliciesCommand {
            principal_hrn: Hrn::from_string("hrn:aws:iam::account-a:User/alice").unwrap(),
            action_name: "CreateUser".to_string(),
            resource_hrn: Hrn::from_string("hrn:aws:iam::account-a:User/bob").unwrap(),
        };

        let json = serde_json::to_string(&command).expect("the command serializes");
        let parsed: EvaluateIamPoliciesCommand =
            serde_json::from_str(&json).expect("the command deserializes");

        assert_eq!(parsed.principal_hrn, command.principal_hrn);
        assert_eq!(parsed.action_name, command.action_name);
        assert_eq!(parsed.resource_hrn, command.resource_hrn);
    }
}
