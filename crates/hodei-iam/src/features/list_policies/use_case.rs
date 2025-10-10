//! Use Case: List Policies

use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info, instrument};

use super::dto::{ListPoliciesQuery, ListPoliciesResponse};
use super::error::ListPoliciesError;
use super::ports::{ListPoliciesUseCasePort, PolicyLister};

/// Use case forlisting IAM policies with pagination
///
/// This use case orchestrates the listing of policies:
/// 1. Validates the pagination parameters
/// 2. Delegates the query to the persistence port
/// 3. Returns the response with pagination metadata
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::features::list_policies::{ListPoliciesQuery, ListPoliciesUseCase};
/// use std::sync::Arc;
///
/// let lister = Arc::new(InMemoryPolicyLister::new());
/// let use_case = ListPoliciesUseCase::new(lister);
///
/// let query = ListPoliciesQuery::with_pagination(20, 0);
/// let response = use_case.execute(query).await?;
///
/// println!("Found {} policies", response.policies.len());
///if response.has_next_page {
///     println!("There are more pages available");
/// }
/// ```
pub struct ListPoliciesUseCase {
    /// Port for listing policies
    lister: Arc<dyn PolicyLister>,
}

impl ListPoliciesUseCase {
    /// Create a new instance of the use case
    ///
    /// # Arguments
    ///
    /// * `lister` - Implementation of `PolicyLister` for data retrieval
    pub fn new(lister: Arc<dyn PolicyLister>) -> Self {
        Self { lister }
    }

    /// Execute the list policies use case
    ///
    /// # Arguments
    ///
    /// * `query` - Query with pagination parameters
    ///
    /// # Returns
    ///
    /// On success, returns `Ok(ListPoliciesResponse)` with the list of policies
    /// and pagination metadata.
    ///
    /// # Errors
    ///
    /// - `ListPoliciesError::InvalidPagination` - Invalid pagination parameters
    /// - `ListPoliciesError::RepositoryError` - Database or storage failure
    /// - `ListPoliciesError::InternalError` - Unexpected error
    #[instrument(skip(self), fields(limit = ?query.limit, offset = ?query.offset))]
    pub async fn execute(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<ListPoliciesResponse, ListPoliciesError> {
        info!(
            "Listing policies with limit={} offset={}",
            query.limit, query.offset
        );

        // Validate pagination parameters
        self.validate_pagination(&query)?;

        // Delegate to the port
        let response = self
            .lister
            .list(query)
            .await
            .map_err(|e| ListPoliciesError::RepositoryError(e.to_string()))?;

        debug!(
            "Retrieved {} policies, total_count={}",
            response.policies.len(),
            response.total_count
        );

        Ok(response)
    }

    /// Validate pagination parameters
    fn validate_pagination(&self, query: &ListPoliciesQuery) -> Result<(), ListPoliciesError> {
        if query.limit == 0 {
            return Err(ListPoliciesError::InvalidPagination(
                "Limit must be greater than 0".to_string(),
            ));
        }

        if query.limit > 100 {
            return Err(ListPoliciesError::InvalidPagination(
                "Limit must be less than or equal to 100".to_string(),
            ));
        }

        Ok(())
    }
}

// Implement PolicyLister trait for the use case to enable trait object usage
#[async_trait]
impl PolicyLister for ListPoliciesUseCase {
    async fn list(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<ListPoliciesResponse, ListPoliciesError> {
        self.execute(query).await
    }
}

#[async_trait]
impl ListPoliciesUseCasePort for ListPoliciesUseCase {
    async fn execute(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<ListPoliciesResponse, ListPoliciesError> {
        self.execute(query).await
    }
}
