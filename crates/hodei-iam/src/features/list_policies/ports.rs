//! Ports (interfaces) for List Policies feature
//!
//! Following Interface Segregation Principle (ISP),
//! this feature defines only the minimal port it needs.

use async_trait::async_trait;

use super::dto::{ListPoliciesQuery, ListPoliciesResponse};
use super::error::ListPoliciesError;

/// Port for listing policies with pagination
///
/// This port is segregated to only handle listing operations.
/// It does not include create, read, update, or delete operations.
///
/// # Interface Segregation
///
/// By separating the list operation into its own port, we ensure that:
/// - Implementations only need to support pagination and listing
/// - Consumers don't depend on unused operations
/// - The interface can evolve independently
///
/// # Example Implementation
///
/// ```rust,ignore
/// use async_trait::async_trait;
///
/// struct SurrealPolicyLister {
///     db: SurrealClient,
/// }
///
/// #[async_trait]
/// impl PolicyLister for SurrealPolicyLister {
///     async fn list(&self, query: ListPoliciesQuery)
///         -> Result<ListPoliciesResponse, ListPoliciesError> {
///         // Execute paginated query in SurrealDB
///         // Return response with page info
///     }
/// }
/// ```
#[async_trait]
pub trait PolicyLister: Send + Sync {
    /// List policies with pagination
    ///
    /// # Arguments
    ///
    /// * `query` - Query with pagination parameters (limit, offset)
    ///
    /// # Returns
    ///
    /// * `Ok(ListPoliciesResponse)` - List of policies with pagination metadata
    /// * `Err(ListPoliciesError)` - If an error occurs during listing
    ///
    /// # Pagination
    ///
    /// The implementation should:
    /// - Respect the limit (max 100 items per page)
    /// - Apply the offset for page navigation
    /// - Return accurate total_count for has_next_page calculation
    /// - Handle edge cases (offset beyond total, empty results, etc.)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let query = ListPoliciesQuery::with_pagination(20, 40);
    /// let response = lister.list(query).await?;
    ///
    /// println!("Page has {} policies", response.policies.len());
    /// println!("Total: {}", response.page_info.total_count);
    /// if response.page_info.has_next_page {
    ///     println!("Next offset: {:?}", response.page_info.next_offset());
    /// }
    /// ```
    async fn list(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<ListPoliciesResponse, ListPoliciesError>;
}

/// Port for the ListPolicies use case
///
/// This port defines the contract for executing the list policies use case.
/// Following the Interface Segregation Principle (ISP), this port
/// contains only the execute method needed by external callers.
#[async_trait]
pub trait ListPoliciesUseCasePort: Send + Sync {
    /// Execute the list policies use case
    ///
    /// # Arguments
    /// * `query` - The list policies query containing pagination parameters
    ///
    /// # Returns
    /// * `Ok(ListPoliciesResponse)` if the policies were listed successfully
    /// * `Err(ListPoliciesError)` if there was an error listing the policies
    async fn execute(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<ListPoliciesResponse, ListPoliciesError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_lister_is_object_safe() {
        // This test ensures the trait is object-safe (can be used as dyn PolicyLister)
        fn _assert_object_safe(_: &dyn PolicyLister) {}
    }
}
