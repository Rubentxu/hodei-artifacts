//! Ports (interfaces) for Get Policy feature
//!
//! Following Interface Segregation Principle (ISP),
//! this feature defines only the minimal port it needs.

use async_trait::async_trait;
use kernel::Hrn;

use super::dto::{GetPolicyQuery, PolicyView};
use super::error::GetPolicyError;

/// Port for reading a single policy by HRN
#[async_trait]
pub trait PolicyReader: Send + Sync {
    /// Get a policy by its HRN
    ///
    /// # Arguments
    ///
    /// * `hrn` - The HRN of the policy to retrieve
    ///
    /// # Returns
    ///
    /// * `Ok(PolicyView)` - The policy if found
    /// * `Err(GetPolicyError)` - If the policy doesn't exist or an error occurs
    async fn get_by_hrn(&self, hrn: &Hrn) -> Result<PolicyView, GetPolicyError>;
}

/// Port for the GetPolicy use case
///
/// This port defines the contract for executing the get policy use case.
/// Following the Interface Segregation Principle (ISP), this port
/// contains only the execute method needed by external callers.
#[async_trait]
pub trait GetPolicyUseCasePort: Send + Sync {
    /// Execute the get policy use case
    ///
    /// # Arguments
    /// * `query` - The get policy query containing policy HRN
    ///
    /// # Returns
    /// * `Ok(PolicyView)` if the policy was found successfully
    /// * `Err(GetPolicyError)` if there was an error getting the policy
    async fn execute(&self, query: GetPolicyQuery) -> Result<PolicyView, GetPolicyError>;
}
