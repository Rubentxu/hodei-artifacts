use super::dto::{ListPoliciesQuery, ListPoliciesResponse, PolicySummaryDto};
use super::error::ListPoliciesError;
use super::ports::{ListPoliciesConfig, ListQueryValidator, PolicyLister, PolicyListingAuditor, PolicyListingStorage};
use crate::domain::policy::Policy;
use async_trait::async_trait;
use shared::hrn::UserId;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};

/// Use case for listing policies
pub struct ListPoliciesUseCase<QV, PLS, PLA> {
    query_validator: Arc<dyn ListQueryValidator>,
    storage: Arc<dyn PolicyListingStorage>,
    auditor: Arc<dyn PolicyListingAuditor>,
    config: ListPoliciesConfig,
    _phantom: PhantomData<(QV, PLS, PLA)>,
}

impl<QV, PLS, PLA> ListPoliciesUseCase<QV, PLS, PLA> {
    pub fn new(
        query_validator: Arc<dyn ListQueryValidator>,
        storage: Arc<dyn PolicyListingStorage>,
        auditor: Arc<dyn PolicyListingAuditor>,
        config: ListPoliciesConfig,
    ) -> Self {
        Self {
            query_validator,
            storage,
            auditor,
            config,
            _phantom: PhantomData,
        }
    }

    /// Execute the list policies use case
    pub async fn execute(&self, query: ListPoliciesQuery, user_id: &UserId) -> Result<ListPoliciesResponse, ListPoliciesError> {
        let start_time = Instant::now();

        info!("Listing policies with query: {:?}", query);

        // Validate and enhance query with access filters
        let validated_query = self.query_validator.validate_query(&query, user_id).await?;
        let access_filtered_query = self.query_validator.apply_access_filter(&validated_query, user_id).await?;

        // Apply default limits if not specified
        let final_query = self.apply_defaults(access_filtered_query);

        // Get policies from storage
        let policies = self.storage.find_all(final_query.clone()).await?;
        let total_count = self.storage.count(final_query.clone()).await?;

        // Convert to DTOs
        let policy_dtos = self.convert_to_dtos(policies).await;

        // Calculate pagination info
        let has_more = (final_query.offset.unwrap_or(0) + policy_dtos.len()) < total_count;

        // Log the access
        self.auditor.log_policy_list_access(user_id, &final_query, policy_dtos.len()).await?;

        let execution_time = start_time.elapsed().as_millis() as u64;

        info!("Listed {} policies in {}ms", policy_dtos.len(), execution_time);

        Ok(ListPoliciesResponse {
            policies: policy_dtos,
            total_count,
            has_more,
            query: final_query,
        })
    }

    fn apply_defaults(&self, mut query: ListPoliciesQuery) -> ListPoliciesQuery {
        if query.limit.is_none() {
            query.limit = Some(self.config.default_limit);
        } else if query.limit.unwrap() > self.config.max_limit {
            query.limit = Some(self.config.max_limit);
        }

        if query.offset.is_none() {
            query.offset = Some(0);
        }

        // Validate offset doesn't exceed max
        if let Some(offset) = query.offset {
            if offset > self.config.max_offset {
                query.offset = Some(self.config.max_offset);
            }
        }

        // Set default sorting if not specified
        if query.sort_by.is_none() {
            query.sort_by = Some("updated_at".to_string());
        }

        if query.sort_order.is_none() {
            query.sort_order = Some("desc".to_string());
        }

        query
    }

    async fn convert_to_dtos(&self, policies: Vec<Policy>) -> Vec<PolicySummaryDto> {
        policies
            .into_iter()
            .map(|policy| PolicySummaryDto {
                policy_id: policy.id.to_string(),
                name: policy.name,
                description: policy.description,
                status: policy.status,
                version: policy.version,
                created_at: policy.created_at.to_rfc3339(),
                updated_at: policy.updated_at.to_rfc3339(),
                created_by: policy.current_version.created_by.to_string(),
            })
            .collect()
    }
}

#[async_trait]
impl<QV, PLS, PLA> PolicyLister for ListPoliciesUseCase<QV, PLS, PLA>
where
    QV: ListQueryValidator + Send + Sync,
    PLS: PolicyListingStorage + Send + Sync,
    PLA: PolicyListingAuditor + Send + Sync,
{
    async fn list_policies(&self, query: ListPoliciesQuery, user_id: &UserId) -> Result<Vec<Policy>, ListPoliciesError> {
        // Execute the use case and return the policies
        let response = self.execute(query, user_id).await?;
        Ok(response.policies.into_iter().map(|dto| {
            // Convert back to Policy (simplified - would need full conversion in real implementation)
            Policy {
                id: dto.policy_id.parse().unwrap_or_default(),
                name: dto.name,
                description: dto.description,
                status: dto.status,
                version: dto.version,
                created_at: chrono::DateTime::parse_from_rfc3339(&dto.created_at)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(&dto.updated_at)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                current_version: crate::domain::policy::PolicyVersion {
                    id: shared::hrn::Hrn::new(&format!("{}/versions/{}", dto.policy_id, dto.version)).unwrap(),
                    policy_id: dto.policy_id.parse().unwrap_or_default(),
                    version: dto.version,
                    content: "".to_string(), // Would need to retrieve from storage
                    created_at: chrono::DateTime::parse_from_rfc3339(&dto.created_at)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    created_by: dto.created_by.parse().unwrap_or_default(),
                },
            }
        }).collect())
    }
}
