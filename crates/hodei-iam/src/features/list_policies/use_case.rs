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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::list_policies::dto::PolicySummary;
    use crate::features::list_policies::mocks::MockPolicyLister;
    use kernel::Hrn;

    fn create_test_policy(id: &str) -> PolicySummary {
        PolicySummary {
            hrn: Hrn::new(
                "aws".to_string(),
                "iam".to_string(),
                "123456789012".to_string(),
                "Policy".to_string(),
                id.to_string(),
            ),
            name: format!("Policy {}", id),
            description: Some(format!("Test policy {}", id)),
        }
    }

    #[tokio::test]
    async fn test_list_policies_success() {
        // Arrange
        let policies = vec![
            create_test_policy("policy1"),
            create_test_policy("policy2"),
            create_test_policy("policy3"),
        ];
        let lister = MockPolicyLister::with_policies(policies);
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));

        // Act
        let query = ListPoliciesQuery::with_limit(10);
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 3);
        assert_eq!(response.total_count, 3);
    }

    #[tokio::test]
    async fn test_list_policies_with_pagination() {
        // Arrange
        let mut policies = vec![];
        for i in 0..25 {
            policies.push(create_test_policy(&format!("policy{}", i)));
        }
        let lister = MockPolicyLister::with_policies(policies);
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));

        // Act -First page
        let query = ListPoliciesQuery::with_pagination(10, 0);
        let result = use_case.execute(query).await;

        // Assert - First page
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 10);
        assert_eq!(response.total_count, 25);
        assert!(response.has_next_page);
        assert!(!response.has_previous_page);

        // Act - Second page
        let query = ListPoliciesQuery::with_pagination(10, 10);
        let result = use_case.execute(query).await;

        // Assert - Second page
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 10);
        assert!(response.has_next_page);
        assert!(response.has_previous_page);
    }

    #[tokio::test]
    async fn test_list_policies_empty() {
        // Arrange
        let lister = MockPolicyLister::empty();
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));

        // Act
        let query = ListPoliciesQuery::default();
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.policies.is_empty());
        assert_eq!(response.total_count, 0);
        assert!(!response.has_next_page);
    }

    #[tokio::test]
    async fn test_list_policies_invalid_limit() {
        // Arrange
        let lister = MockPolicyLister::empty();
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));

        // Act
        let query = ListPoliciesQuery {
            limit: 0,
            offset: 0,
        };
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            ListPoliciesError::InvalidPagination(_) => {}
            e => panic!("Expected InvalidPagination, got:{:?}", e),
        }
    }

    #[tokio::test]
    async fn test_list_policies_repository_error() {
        // Arrange
        let lister = MockPolicyLister::with_error();
        let use_case = ListPoliciesUseCase::new(Arc::new(lister));

        // Act
        let query = ListPoliciesQuery::default();
        let result = use_case.execute(query).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            ListPoliciesError::RepositoryError(_) => {}
            e => panic!("Expected RepositoryError, got: {:?}", e),
        }
    }
}
