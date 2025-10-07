//! SurrealDB implementation of OrganizationBoundaryProvider
//!
//! This module provides the infrastructure adapter that resolves effective
//! Service Control Policies (SCPs) for resources in the organizational hierarchy.
//!
//! ## Architecture
//!
//! This implementation follows Clean Architecture principles by:
//! - Injecting repository dependencies (not creating them internally)
//! - Implementing business logic directly (not delegating to use cases)
//! - Using domain repositories from the application layer
//!
//! ## Algorithm
//!
//! See `docs/historias/HISTORIA-4-ALGORITMO.md` for detailed algorithm specification.

use async_trait::async_trait;
use cedar_policy::{Policy, PolicyId, PolicySet};
use kernel::Hrn;
use std::collections::HashSet;
use tracing::{debug, error, info, warn};

use crate::features::evaluate_permissions::error::EvaluatePermissionsError;
use crate::features::evaluate_permissions::ports::OrganizationBoundaryProvider;
use hodei_organizations::internal_api::application::ports::account_repository::AccountRepository;
use hodei_organizations::internal_api::application::ports::ou_repository::OuRepository;
use hodei_organizations::internal_api::application::ports::scp_repository::ScpRepository;

/// SurrealDB implementation of OrganizationBoundaryProvider
///
/// This adapter resolves effective SCPs by traversing the organizational
/// hierarchy using injected repositories.
///
/// # Type Parameters
///
/// * `SR` - ScpRepository implementation
/// * `AR` - AccountRepository implementation
/// * `OR` - OuRepository implementation
///
/// # Example
///
/// ```rust,ignore
/// let provider = SurrealOrganizationBoundaryProvider::new(
///     scp_repository,
///     account_repository,
///     ou_repository,
/// );
///
/// let policy_set = provider
///     .get_effective_scps_for(&account_hrn)
///     .await?;
/// ```
pub struct SurrealOrganizationBoundaryProvider<SR, AR, OR>
where
    SR: ScpRepository + Send + Sync,
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    scp_repository: SR,
    account_repository: AR,
    ou_repository: OR,
}

impl<SR, AR, OR> SurrealOrganizationBoundaryProvider<SR, AR, OR>
where
    SR: ScpRepository + Send + Sync,
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    /// Create a new SurrealOrganizationBoundaryProvider with injected repositories
    ///
    /// # Arguments
    ///
    /// * `scp_repository` - Repository for Service Control Policies
    /// * `account_repository` - Repository for Accounts
    /// * `ou_repository` - Repository for Organizational Units
    pub fn new(scp_repository: SR, account_repository: AR, ou_repository: OR) -> Self {
        Self {
            scp_repository,
            account_repository,
            ou_repository,
        }
    }

    /// Classify the resource type from HRN
    fn classify_resource_type(hrn: &Hrn) -> Result<ResourceType, EvaluatePermissionsError> {
        match hrn.resource_type.to_lowercase().as_str() {
            "account" => Ok(ResourceType::Account),
            "organizationalunit" | "ou" => Ok(ResourceType::OrganizationalUnit),
            other => Err(EvaluatePermissionsError::OrganizationBoundaryProviderError(
                format!("Invalid target type: {}", other),
            )),
        }
    }

    /// Resolve SCPs starting from an Account
    async fn resolve_from_account(
        &self,
        hrn: &Hrn,
    ) -> Result<(HashSet<Hrn>, Option<Hrn>), EvaluatePermissionsError> {
        let account = self
            .account_repository
            .find_by_hrn(hrn)
            .await
            .map_err(|e| {
                EvaluatePermissionsError::OrganizationBoundaryProviderError(format!(
                    "Failed to load account: {}",
                    e
                ))
            })?
            .ok_or_else(|| {
                EvaluatePermissionsError::OrganizationBoundaryProviderError(format!(
                    "Account not found: {}",
                    hrn
                ))
            })?;

        let scps = account.attached_scps.clone();
        let parent_ou_hrn = account.parent_hrn.clone();

        Ok((scps, parent_ou_hrn))
    }

    /// Resolve SCPs starting from an OU
    async fn resolve_from_ou(
        &self,
        hrn: &Hrn,
    ) -> Result<(HashSet<Hrn>, Option<Hrn>), EvaluatePermissionsError> {
        let ou = self
            .ou_repository
            .find_by_hrn(hrn)
            .await
            .map_err(|e| {
                EvaluatePermissionsError::OrganizationBoundaryProviderError(format!(
                    "Failed to load OU: {}",
                    e
                ))
            })?
            .ok_or_else(|| {
                EvaluatePermissionsError::OrganizationBoundaryProviderError(format!(
                    "Organizational Unit not found: {}",
                    hrn
                ))
            })?;

        let scps = ou.attached_scps.clone();

        Ok((scps, Some(hrn.clone())))
    }

    /// Collect SCPs by traversing the OU hierarchy upward
    ///
    /// This method implements an iterative ascent through the OU tree,
    /// accumulating SCPs at each level until reaching the root.
    ///
    /// # Cycle Detection
    ///
    /// Uses a visited set to detect and prevent infinite loops.
    async fn collect_scps_from_hierarchy(
        &self,
        start_ou_hrn: Option<Hrn>,
    ) -> Result<HashSet<Hrn>, EvaluatePermissionsError> {
        let mut accumulated_scps = HashSet::new();
        let mut visited = HashSet::new();
        let mut current_ou_hrn = start_ou_hrn;

        while let Some(ref ou_hrn) = current_ou_hrn {
            debug!("Processing OU in hierarchy: {}", ou_hrn);
            // Cycle detection
            if visited.contains(ou_hrn) {
                error!("Cycle detected in OU hierarchy at: {}", ou_hrn);
                return Err(EvaluatePermissionsError::OrganizationBoundaryProviderError(
                    format!("Cycle detected in OU hierarchy at: {}", ou_hrn),
                ));
            }

            visited.insert(ou_hrn.clone());

            // Load current OU
            let ou = self.ou_repository.find_by_hrn(ou_hrn).await.map_err(|e| {
                EvaluatePermissionsError::OrganizationBoundaryProviderError(format!(
                    "Failed to load OU during hierarchy traversal: {}",
                    e
                ))
            })?;

            // If OU not found, assume we've reached beyond the root
            let Some(ou) = ou else {
                warn!(
                    "OU referenced but not found: {}, assuming root reached",
                    ou_hrn
                );
                break;
            };

            // Accumulate SCPs from this level
            debug!("OU {} has {} attached SCPs", ou_hrn, ou.attached_scps.len());
            accumulated_scps.extend(ou.attached_scps.iter().cloned());
            debug!("Total accumulated SCPs: {}", accumulated_scps.len());

            // Check if we've reached the root
            // Root detection: parent_hrn points to itself or parent doesn't exist
            debug!("OU parent_hrn: {}", ou.parent_hrn);
            if &ou.parent_hrn == ou_hrn {
                // Root OU points to itself
                debug!("Root detected (self-reference), stopping hierarchy traversal");
                break;
            }

            // Try to load parent to verify it exists
            debug!("Checking if parent OU exists: {}", ou.parent_hrn);
            let parent_exists = self
                .ou_repository
                .find_by_hrn(&ou.parent_hrn)
                .await
                .map_err(|e| {
                    EvaluatePermissionsError::OrganizationBoundaryProviderError(format!(
                        "Failed to check parent OU existence: {}",
                        e
                    ))
                })?
                .is_some();

            debug!("Parent OU exists: {}", parent_exists);
            if !parent_exists {
                // Parent doesn't exist, we've reached the root
                debug!("Parent OU doesn't exist, stopping hierarchy traversal");
                break;
            }

            // Move to parent
            debug!("Moving to parent OU: {}", ou.parent_hrn);
            current_ou_hrn = Some(ou.parent_hrn.clone());
        }

        Ok(accumulated_scps)
    }

    /// Load SCP policies and construct a Cedar PolicySet
    ///
    /// This method loads each SCP by HRN, parses its policy document,
    /// and adds it to the PolicySet. Malformed policies are logged
    /// but do not abort the entire operation.
    async fn load_policy_set(
        &self,
        scp_hrns: HashSet<Hrn>,
    ) -> Result<PolicySet, EvaluatePermissionsError> {
        let mut policy_set = PolicySet::new();

        // Sort HRNs for deterministic ordering (helps testing)
        let mut sorted_hrns: Vec<_> = scp_hrns.into_iter().collect();
        sorted_hrns.sort_by_key(|a| a.to_string());

        debug!(
            "Loading and parsing {} SCPs into PolicySet",
            sorted_hrns.len()
        );

        for scp_hrn in sorted_hrns {
            debug!("Loading SCP: {}", scp_hrn);
            // Load SCP from repository
            let scp = self
                .scp_repository
                .find_by_hrn(&scp_hrn)
                .await
                .map_err(|e| {
                    EvaluatePermissionsError::OrganizationBoundaryProviderError(format!(
                        "Failed to load SCP {}: {}",
                        scp_hrn, e
                    ))
                })?;

            // If SCP not found, log warning and continue
            let Some(scp) = scp else {
                warn!("SCP referenced but not found: {}", scp_hrn);
                continue;
            };

            debug!("Found SCP, parsing Cedar policy document");

            // Parse policy document with unique ID based on SCP HRN
            // This ensures each SCP gets a unique PolicyId in the PolicySet
            let policy_id = PolicyId::new(scp_hrn.to_string());
            match Policy::parse(Some(policy_id), &scp.document) {
                Ok(policy) => {
                    debug!("Successfully parsed policy for SCP: {}", scp_hrn);
                    let _ = policy_set.add(policy);
                    debug!("Added policy to PolicySet");
                }
                Err(e) => {
                    warn!("Failed to parse SCP policy {}: {}. Skipping.", scp_hrn, e);
                    // Continue with other policies
                }
            }
        }

        Ok(policy_set)
    }
}

#[async_trait]
impl<SR, AR, OR> OrganizationBoundaryProvider for SurrealOrganizationBoundaryProvider<SR, AR, OR>
where
    SR: ScpRepository + Send + Sync,
    AR: AccountRepository + Send + Sync,
    OR: OuRepository + Send + Sync,
{
    /// Get effective SCPs for a resource (Account or OU)
    ///
    /// This implementation:
    /// 1. Classifies the resource type from the HRN
    /// 2. Resolves the initial SCPs and entry point
    /// 3. Traverses the OU hierarchy collecting SCPs
    /// 4. Loads and parses all accumulated SCP policies
    /// 5. Returns a Cedar PolicySet
    ///
    /// # Algorithm
    ///
    /// See `docs/historias/HISTORIA-4-ALGORITMO.md` for detailed specification.
    async fn get_effective_scps_for(
        &self,
        resource_hrn: &Hrn,
    ) -> Result<PolicySet, EvaluatePermissionsError> {
        info!("Starting SCP resolution for resource: {}", resource_hrn);

        // Step 1: Classify resource type
        let resource_type = Self::classify_resource_type(resource_hrn)?;

        // Step 2: Resolve entry point and initial SCPs
        let (initial_scps, start_ou_hrn) = match resource_type {
            ResourceType::Account => self.resolve_from_account(resource_hrn).await?,
            ResourceType::OrganizationalUnit => self.resolve_from_ou(resource_hrn).await?,
        };

        // Step 3: Accumulate initial SCPs
        let mut accumulated_scps = initial_scps;

        // Step 4: Traverse hierarchy if there's a parent OU
        if let Some(ou_hrn) = start_ou_hrn {
            let hierarchy_scps = self.collect_scps_from_hierarchy(Some(ou_hrn)).await?;
            accumulated_scps.extend(hierarchy_scps);
        }

        // Step 5: Load and parse policies
        let policy_set = self.load_policy_set(accumulated_scps).await?;

        info!(
            "Resolved {} effective SCPs for resource: {}",
            policy_set.policies().count(),
            resource_hrn
        );

        Ok(policy_set)
    }
}

/// Resource type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceType {
    Account,
    OrganizationalUnit,
}
