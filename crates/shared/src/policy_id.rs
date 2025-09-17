//! PolicyId type for identifying policies in the system.
//!
//! This module defines the PolicyId type which is used to uniquely identify policies
//! throughout the system. PolicyId is based on HRN (Hierarchical Resource Names) to
//! ensure global uniqueness and provide organizational context.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::hrn::{Hrn, OrganizationId};

/// HRN-based identifier for policies.
///
/// PolicyId follows the HRN format: `hrn:hodei:iam::<org>:policy/<policy-name>`
/// This ensures global uniqueness and provides clear organizational context.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PolicyId(pub Hrn);

impl PolicyId {
    /// Create a new PolicyId from an organization and policy name.
    ///
    /// # Arguments
    ///
    /// * `org_id` - The organization that owns the policy
    /// * `policy_name` - The name of the policy
    ///
    /// # Returns
    ///
    /// A Result containing the new PolicyId or an HrnError if the inputs are invalid
    ///
    /// # Example
    ///
    /// ```
    /// use shared::hrn::{OrganizationId, HodeiPolicyId};
    ///
    /// let org_id = OrganizationId::new("acme-corp").unwrap();
    /// let policy_id = HodeiPolicyId::new(&org_id, "standard-user-policy");
    /// assert!(policy_id.is_ok());
    /// ```
    pub fn new(org_id: &OrganizationId, policy_name: &str) -> Result<Self, HrnError> {
        let hrn = Hrn::new(&format!("{}/policy/{}", org_id.as_str(), policy_name))?;
        Ok(PolicyId(hrn))
    }
    
    /// Get the string representation of the PolicyId.
    ///
    /// # Returns
    ///
    /// A string slice containing the full HRN of the policy
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for PolicyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for PolicyId {
    type Err = HrnError;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PolicyId(Hrn::new(s)?))
    }
}