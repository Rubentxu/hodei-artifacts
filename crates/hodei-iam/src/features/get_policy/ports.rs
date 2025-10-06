//! Ports (interfaces) for Get Policy feature
//!
//! Following Interface Segregation Principle (ISP),
//! this feature defines only the minimal port it needs.

use async_trait::async_trait;
use kernel::Hrn;

use super::dto::PolicyView;
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

