#[cfg(test)]
mod tests {
    use super::super::dto::{AuthorizationDecision, AuthorizationRequest, AuthorizationResponse};
    use super::super::error::EvaluatePermissionsError;
    use super::super::mocks::{
        MockAuthorizationCache, MockAuthorizationLogger, MockAuthorizationMetrics,
        MockEntityResolver,
    };
    use super::super::use_case::EvaluatePermissionsUseCase;
    use async_trait::async_trait;
    use cedar_policy::{Context, Entities, EntityUid, PolicySet, Request};
    use kernel::application::ports::{
        EffectivePoliciesQuery, EffectivePoliciesQueryPort, EffectivePoliciesResult,
        GetEffectiveScpsPort, GetEffectiveScpsQuery,
    };
    use policies::shared::AuthorizationEngine;
    use std::sync::Arc;

    // Mock IAM port that returns empty policies
    struct MockIamPort;

    #[async_trait]
    impl EffectivePoliciesQueryPort for MockIamPort {
        async fn get_effective_policies(
            &self,
            _query: EffectivePoliciesQuery,
        ) -> Result<EffectivePoliciesResult, Box<dyn std::error::Error + Send + Sync>> {
            Ok(EffectivePoliciesResult {
                policies: PolicySet::new(),
                policy_count: 0,
            })
        }
    }

    // Mock IAM port that returns a permit policy
    struct MockIamPortWithPermit;

    #[async_trait]
    impl EffectivePoliciesQueryPort for MockIamPortWithPermit {
        async fn get_effective_policies(
            &self,
            _query: EffectivePoliciesQuery,
        ) -> Result<EffectivePoliciesResult, Box<dyn std::error::Error + Send + Sync>> {
            let policy_text = r#"permit(principal, action, resource);"#;
            let policy = policy_text.parse::<cedar_policy::Policy>().unwrap();
            let mut policy_set = PolicySet::new();
            policy_set.add(policy).unwrap();

            Ok(EffectivePoliciesResult {
                policies: policy_set,
                policy_count: 1,
            })
        }
    }

    // Mock IAM port that returns a forbid policy
    struct MockIamPortWithForbid;

    #[async_trait]
    impl EffectivePoliciesQueryPort for MockIamPortWithForbid {
        async fn get_effective_policies(
            &self,
            _query: EffectivePoliciesQuery,
        ) -> Result<EffectivePoliciesResult, Box<dyn std::error::Error + Send + Sync>> {
            let policy_text = r#"forbid(principal, action, resource);"#;
            let policy = policy_text.parse::<cedar_policy::Policy>().unwrap();
            let mut policy_set = PolicySet::new();
            policy_set.add(policy).unwrap();

            Ok(EffectivePoliciesResult {
                policies: policy_set,
                policy_count: 1,
            })
        }
    }

    // Mock SCP port that returns empty policies
    struct MockScpPort;

    #[async_trait]
    impl GetEffectiveScpsPort for MockScpPort {
        async fn get_effective_scps(
            &self,
            _query: GetEffectiveScpsQuery,
        ) -> Result<PolicySet, Box<dyn std::error::Error + Send + Sync>> {
            Ok(PolicySet::new())
        }
    }

    // Mock IAM port that returns error
    struct MockIamPortWithError;

    #[async_trait]
    impl EffectivePoliciesQueryPort for MockIamPortWithError {
        async fn get_effective_policies(
            &self,
            _query: EffectivePoliciesQuery,
        ) -> Result<EffectivePoliciesResult, Box<dyn std::error::Error + Send + Sync>> {
            Err("IAM query failed".into())
        }
    }

    fn create_test_engine() -> Arc<AuthorizationEngine> {
        Arc::new(AuthorizationEngine::new(
            PolicySet::new(),
            Entities::empty(),
        ))
    }

    #[tokio::test]
    async fn test_evaluate_with_empty_policies_denies() {
        // Arrange
        let iam_port = Arc::new(MockIamPort);
        let org_port: Option<Arc<dyn GetEffectiveScpsPort>> = None;
        let engine = create_test_engine();
        let entity_resolver = Arc::new(MockEntityResolver);
        let cache = Some(MockAuthorizationCache::new());
        let logger = MockAuthorizationLogger;
        let metrics = MockAuthorizationMetrics;

        let use_case = EvaluatePermissionsUseCase::new(
            iam_port,
            org_port,
            engine,
            entity_resolver,
            cache,
            logger,
            metrics,
        );

        let request = AuthorizationRequest {
            principal: "User::\"alice\"".to_string(),
            action: "Action::\"read\"".to_string(),
            resource: "Resource::\"doc1\"".to_string(),
            context: None,
        };

        // Act
        let result = use_case.execute(request).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.decision, AuthorizationDecision::Deny);
        assert!(
            response.reasons.is_empty()
                || response
                    .reasons
                    .contains(&"No matching permit policy found".to_string())
        );
    }

    #[tokio::test]
    async fn test_evaluate_with_permit_policy_allows() {
        // Arrange
        let iam_port = Arc::new(MockIamPortWithPermit);
        let org_port: Option<Arc<dyn GetEffectiveScpsPort>> = None;
        let engine = create_test_engine();
        let entity_resolver = Arc::new(MockEntityResolver);
        let cache = Some(MockAuthorizationCache::new());
        let logger = MockAuthorizationLogger;
        let metrics = MockAuthorizationMetrics;

        let use_case = EvaluatePermissionsUseCase::new(
            iam_port,
            org_port,
            engine,
            entity_resolver,
            cache,
            logger,
            metrics,
        );

        let request = AuthorizationRequest {
            principal: "User::\"alice\"".to_string(),
            action: "Action::\"read\"".to_string(),
            resource: "Resource::\"doc1\"".to_string(),
            context: None,
        };

        // Act
        let result = use_case.execute(request).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.decision, AuthorizationDecision::Allow);
    }

    #[tokio::test]
    async fn test_evaluate_with_forbid_policy_denies() {
        // Arrange
        let iam_port = Arc::new(MockIamPortWithForbid);
        let org_port: Option<Arc<dyn GetEffectiveScpsPort>> = None;
        let engine = create_test_engine();
        let entity_resolver = Arc::new(MockEntityResolver);
        let cache = Some(MockAuthorizationCache::new());
        let logger = MockAuthorizationLogger;
        let metrics = MockAuthorizationMetrics;

        let use_case = EvaluatePermissionsUseCase::new(
            iam_port,
            org_port,
            engine,
            entity_resolver,
            cache,
            logger,
            metrics,
        );

        let request = AuthorizationRequest {
            principal: "User::\"alice\"".to_string(),
            action: "Action::\"read\"".to_string(),
            resource: "Resource::\"doc1\"".to_string(),
            context: None,
        };

        // Act
        let result = use_case.execute(request).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.decision, AuthorizationDecision::Deny);
    }

    #[tokio::test]
    async fn test_evaluate_with_scp_integration() {
        // Arrange
        let iam_port = Arc::new(MockIamPortWithPermit);
        let org_port: Option<Arc<dyn GetEffectiveScpsPort>> = Some(Arc::new(MockScpPort));
        let engine = create_test_engine();
        let entity_resolver = Arc::new(MockEntityResolver);
        let cache = Some(MockAuthorizationCache::new());
        let logger = MockAuthorizationLogger;
        let metrics = MockAuthorizationMetrics;

        let use_case = EvaluatePermissionsUseCase::new(
            iam_port,
            org_port,
            engine,
            entity_resolver,
            cache,
            logger,
            metrics,
        );

        let request = AuthorizationRequest {
            principal: "User::\"alice\"".to_string(),
            action: "Action::\"read\"".to_string(),
            resource: "Resource::\"doc1\"".to_string(),
            context: None,
        };

        // Act
        let result = use_case.execute(request).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        // With permit policy and no SCP restrictions, should allow
        assert_eq!(response.decision, AuthorizationDecision::Allow);
    }

    #[tokio::test]
    async fn test_evaluate_handles_iam_error() {
        // Arrange
        let iam_port = Arc::new(MockIamPortWithError);
        let org_port: Option<Arc<dyn GetEffectiveScpsPort>> = None;
        let engine = create_test_engine();
        let entity_resolver = Arc::new(MockEntityResolver);
        let cache = Some(MockAuthorizationCache::new());
        let logger = MockAuthorizationLogger;
        let metrics = MockAuthorizationMetrics;

        let use_case = EvaluatePermissionsUseCase::new(
            iam_port,
            org_port,
            engine,
            entity_resolver,
            cache,
            logger,
            metrics,
        );

        let request = AuthorizationRequest {
            principal: "User::\"alice\"".to_string(),
            action: "Action::\"read\"".to_string(),
            resource: "Resource::\"doc1\"".to_string(),
            context: None,
        };

        // Act
        let result = use_case.execute(request).await;

        // Assert
        assert!(result.is_err());
        match result.unwrap_err() {
            EvaluatePermissionsError::PolicyRetrievalFailed(_) => {
                // Expected error type
            }
            _ => panic!("Expected PolicyRetrievalFailed error"),
        }
    }

    #[tokio::test]
    async fn test_evaluate_without_cache() {
        // Arrange
        let iam_port = Arc::new(MockIamPortWithPermit);
        let org_port: Option<Arc<dyn GetEffectiveScpsPort>> = None;
        let engine = create_test_engine();
        let entity_resolver = Arc::new(MockEntityResolver);
        let cache: Option<MockAuthorizationCache> = None;
        let logger = MockAuthorizationLogger;
        let metrics = MockAuthorizationMetrics;

        let use_case = EvaluatePermissionsUseCase::new(
            iam_port,
            org_port,
            engine,
            entity_resolver,
            cache,
            logger,
            metrics,
        );

        let request = AuthorizationRequest {
            principal: "User::\"alice\"".to_string(),
            action: "Action::\"read\"".to_string(),
            resource: "Resource::\"doc1\"".to_string(),
            context: None,
        };

        // Act
        let result = use_case.execute(request).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.decision, AuthorizationDecision::Allow);
    }

    #[tokio::test]
    async fn test_evaluate_with_context() {
        // Arrange
        let iam_port = Arc::new(MockIamPortWithPermit);
        let org_port: Option<Arc<dyn GetEffectiveScpsPort>> = None;
        let engine = create_test_engine();
        let entity_resolver = Arc::new(MockEntityResolver);
        let cache = Some(MockAuthorizationCache::new());
        let logger = MockAuthorizationLogger;
        let metrics = MockAuthorizationMetrics;

        let use_case = EvaluatePermissionsUseCase::new(
            iam_port,
            org_port,
            engine,
            entity_resolver,
            cache,
            logger,
            metrics,
        );

        let request = AuthorizationRequest {
            principal: "User::\"alice\"".to_string(),
            action: "Action::\"read\"".to_string(),
            resource: "Resource::\"doc1\"".to_string(),
            context: Some(serde_json::json!({"ip": "192.168.1.1"})),
        };

        // Act
        let result = use_case.execute(request).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.decision, AuthorizationDecision::Allow);
    }

    #[tokio::test]
    async fn test_cache_key_generation_consistency() {
        // Arrange
        let iam_port = Arc::new(MockIamPort);
        let org_port: Option<Arc<dyn GetEffectiveScpsPort>> = None;
        let engine = create_test_engine();
        let entity_resolver = Arc::new(MockEntityResolver);
        let cache = Some(MockAuthorizationCache::new());
        let logger = MockAuthorizationLogger;
        let metrics = MockAuthorizationMetrics;

        let use_case = EvaluatePermissionsUseCase::new(
            iam_port,
            org_port,
            engine,
            entity_resolver,
            cache,
            logger,
            metrics,
        );

        let request1 = AuthorizationRequest {
            principal: "User::\"alice\"".to_string(),
            action: "Action::\"read\"".to_string(),
            resource: "Resource::\"doc1\"".to_string(),
            context: None,
        };

        let request2 = AuthorizationRequest {
            principal: "User::\"alice\"".to_string(),
            action: "Action::\"read\"".to_string(),
            resource: "Resource::\"doc1\"".to_string(),
            context: None,
        };

        // Act - Execute same request twice
        let result1 = use_case.execute(request1).await;
        let result2 = use_case.execute(request2).await;

        // Assert - Both should succeed
        assert!(result1.is_ok());
        assert!(result2.is_ok());

        // Both should have same decision (cached result)
        assert_eq!(result1.unwrap().decision, result2.unwrap().decision);
    }
}
