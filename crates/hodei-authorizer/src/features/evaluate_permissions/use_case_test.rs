use crate::features::evaluate_permissions::dto::{AuthorizationRequest, AuthorizationResponse, AuthorizationDecision, AuthorizationContext};
use crate::features::evaluate_permissions::use_case::EvaluatePermissionsUseCase;
use crate::features::evaluate_permissions::mocks::{
    MockIamPolicyProvider, MockOrganizationBoundaryProvider, MockAuthorizationCache,
    MockAuthorizationLogger, MockAuthorizationMetrics, MockEntityResolver, test_helpers
};
use crate::features::evaluate_permissions::error::EvaluatePermissionsError;

#[tokio::test]
async fn test_evaluate_permissions_allow_with_iam_policy() {
    // Arrange
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    let allow_policy = test_helpers::create_test_policy_set(&[test_helpers::create_allow_all_policy()]);
    
    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(&principal.to_string(), allow_policy);
    
    let org_provider = MockOrganizationBoundaryProvider::new();
    let cache = MockAuthorizationCache::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal.clone(), action.clone(), resource.clone());

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision, AuthorizationDecision::Allow);
    assert!(response.explicit);
}

#[tokio::test]
async fn test_evaluate_permissions_deny_by_scp() {
    // Arrange
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    let allow_policy = test_helpers::create_test_policy_set(&[test_helpers::create_allow_all_policy()]);
    let deny_scp = test_helpers::create_test_scp(
        "scp-1",
        "Deny All",
        test_helpers::create_deny_all_policy()
    );

    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(&principal.to_string(), allow_policy);
    
    let org_provider = MockOrganizationBoundaryProvider::new()
        .with_scps(&principal.to_string(), vec![deny_scp]);
    
    let cache = MockAuthorizationCache::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal, action, resource);

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision, AuthorizationDecision::Deny);
    assert!(response.reason.contains("SCP policy"));
}

#[tokio::test]
async fn test_evaluate_permissions_implicit_deny_no_policies() {
    // Arrange
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    let iam_provider = MockIamPolicyProvider::new(); // No policies
    let org_provider = MockOrganizationBoundaryProvider::new(); // No SCPs
    let cache = MockAuthorizationCache::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal, action, resource);

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision, AuthorizationDecision::Deny);
    assert!(!response.explicit); // Implicit deny
    assert!(response.reason.contains("Principle of Least Privilege"));
}

#[tokio::test]
async fn test_evaluate_permissions_explicit_deny_precedence() {
    // Arrange: Test that deny policies have precedence over allow policies
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    // Create policy set with both allow and deny policies
    let allow_policy = test_helpers::create_allow_all_policy();
    let deny_policy = test_helpers::create_deny_all_policy();
    let policy_set = test_helpers::create_test_policy_set(&[&allow_policy, &deny_policy]);

    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(&principal.to_string(), policy_set);
    
    let org_provider = MockOrganizationBoundaryProvider::new();
    let cache = MockAuthorizationCache::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal, action, resource);

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision, AuthorizationDecision::Deny);
    assert!(response.explicit); // Explicit deny
    assert!(response.reason.contains("explicitly denied by IAM policy"));
    assert!(!response.determining_policies.is_empty());
}

#[tokio::test]
async fn test_evaluate_permissions_explicit_allow_no_denies() {
    // Arrange: Test that allow policies work when no deny policies exist
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    let allow_policy = test_helpers::create_allow_all_policy();
    let policy_set = test_helpers::create_test_policy_set(&[&allow_policy]);

    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(&principal.to_string(), policy_set);
    
    let org_provider = MockOrganizationBoundaryProvider::new();
    let cache = MockAuthorizationCache::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal, action, resource);

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision, AuthorizationDecision::Allow);
    assert!(response.explicit); // Explicit allow
    assert!(response.reason.contains("explicitly allowed by IAM policy"));
    assert!(!response.determining_policies.is_empty());
}

#[tokio::test]
async fn test_evaluate_permissions_principle_of_least_privilege() {
    // Arrange: Test that empty PolicySet results in implicit deny
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    // Create empty PolicySet
    let empty_policy_set = test_helpers::create_test_policy_set(&[]);

    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(&principal.to_string(), empty_policy_set);
    
    let org_provider = MockOrganizationBoundaryProvider::new();
    let cache = MockAuthorizationCache::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal, action, resource);

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision, AuthorizationDecision::Deny);
    assert!(!response.explicit); // Implicit deny
    assert!(response.reason.contains("Principle of Least Privilege"));
    assert!(response.determining_policies.is_empty());
}

#[tokio::test]
async fn test_evaluate_permissions_cache_hit() {
    // Arrange
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    let cached_response = AuthorizationResponse::allow(
        vec!["cached-policy".to_string()],
        "Cached decision".to_string(),
    );

    let cache_key = format!("auth:{}:{}:{}", principal, action, resource);
    let cache = MockAuthorizationCache::new()
        .with_cached_response(&cache_key, cached_response.clone());

    let iam_provider = MockIamPolicyProvider::new();
    let org_provider = MockOrganizationBoundaryProvider::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal, action, resource);

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision, cached_response.decision);
    assert_eq!(response.reason, cached_response.reason);
}

#[tokio::test]
async fn test_evaluate_permissions_with_context() {
    // Arrange
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    let context = AuthorizationContext {
        source_ip: Some("192.168.1.100".to_string()),
        user_agent: Some("test-agent/1.0".to_string()),
        request_time: Some(time::OffsetDateTime::now_utc()),
        additional_context: {
            let mut map = std::collections::HashMap::new();
            map.insert("department".to_string(), serde_json::Value::String("engineering".to_string()));
            map
        },
    };

    let allow_policy = test_helpers::create_test_policy_set(&[test_helpers::create_allow_all_policy()]);
    
    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(&principal.to_string(), allow_policy);
    
    let org_provider = MockOrganizationBoundaryProvider::new();
    let cache = MockAuthorizationCache::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::with_context(principal, action, resource, context);

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision, AuthorizationDecision::Allow);
}

#[tokio::test]
async fn test_evaluate_permissions_iam_provider_error() {
    // Arrange
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    let iam_provider = MockIamPolicyProvider::new().with_failure();
    let org_provider = MockOrganizationBoundaryProvider::new();
    let cache = MockAuthorizationCache::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal, action, resource);

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_err());
    let error = result.unwrap_err();
    match error {
        EvaluatePermissionsError::IamPolicyProviderError(msg) => {
            assert_eq!(msg, "Mock failure");
        }
        _ => panic!("Expected IAM policy provider error"),
    }
}

#[tokio::test]
async fn test_evaluate_permissions_org_provider_error() {
    // Arrange
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    let iam_provider = MockIamPolicyProvider::new();
    let org_provider = MockOrganizationBoundaryProvider::new().with_failure();
    let cache = MockAuthorizationCache::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal, action, resource);

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_err());
    let error = result.unwrap_err();
    match error {
        EvaluatePermissionsError::OrganizationBoundaryProviderError(msg) => {
            assert_eq!(msg, "Mock failure");
        }
        _ => panic!("Expected organization boundary provider error"),
    }
}

#[tokio::test]
async fn test_evaluate_permissions_logging_and_metrics() {
    // Arrange
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    let allow_policy = test_helpers::create_test_policy_set(&[test_helpers::create_allow_all_policy()]);
    
    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(&principal.to_string(), allow_policy);
    
    let org_provider = MockOrganizationBoundaryProvider::new();
    let cache = MockAuthorizationCache::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal.clone(), action.clone(), resource.clone());

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_ok());

    // Check logging
    let logged_decisions = logger.get_logged_decisions();
    assert_eq!(logged_decisions.len(), 1);
    let (logged_request, logged_response) = &logged_decisions[0];
    assert_eq!(logged_request.principal, principal);
    assert_eq!(logged_response.decision, AuthorizationDecision::Allow);

    // Check metrics
    let recorded_decisions = metrics.get_recorded_decisions();
    assert_eq!(recorded_decisions.len(), 1);
    let (decision, time_ms) = &recorded_decisions[0];
    assert_eq!(decision, &AuthorizationDecision::Allow);
    assert!(*time_ms > 0); // Should record some evaluation time
}

#[tokio::test]
async fn test_evaluate_permissions_no_cache() {
    // Arrange
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    let allow_policy = test_helpers::create_test_policy_set(&[test_helpers::create_allow_all_policy()]);
    
    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(&principal.to_string(), allow_policy);
    
    let org_provider = MockOrganizationBoundaryProvider::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        None::<MockAuthorizationCache>, // No cache
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal, action, resource);

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision, AuthorizationDecision::Allow);
}

#[tokio::test]
async fn test_evaluate_permissions_specific_policy_match() {
    // Arrange
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    let specific_policy = test_helpers::create_specific_allow_policy(
        &principal.to_string(),
        &action,
        &resource.to_string()
    );
    let policy_set = test_helpers::create_test_policy_set(&[&specific_policy]);
    
    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(&principal.to_string(), policy_set);
    
    let org_provider = MockOrganizationBoundaryProvider::new();
    let cache = MockAuthorizationCache::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal, action, resource);

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.decision, AuthorizationDecision::Allow);
    assert!(response.determining_policies.len() > 0);
}

#[tokio::test]
async fn test_evaluate_permissions_entity_resolver_error() {
    // Arrange
    let principal = test_helpers::create_test_hrn("user", "alice");
    let resource = test_helpers::create_test_hrn("bucket", "test-bucket");
    let action = "read".to_string();

    let allow_policy = test_helpers::create_test_policy_set(&[test_helpers::create_allow_all_policy()]);
    
    let iam_provider = MockIamPolicyProvider::new()
        .with_policy(&principal.to_string(), allow_policy);
    
    let org_provider = MockOrganizationBoundaryProvider::new();
    let cache = MockAuthorizationCache::new();
    let logger = MockAuthorizationLogger::new();
    let metrics = MockAuthorizationMetrics::new();
    let entity_resolver = MockEntityResolver::new().with_failure();

    let use_case = EvaluatePermissionsUseCase::new(
        iam_provider,
        org_provider,
        Some(cache),
        logger,
        metrics,
        entity_resolver,
    );

    let request = AuthorizationRequest::new(principal, action, resource);

    // Act
    let result = use_case.execute(request).await;

    // Assert
    assert!(result.is_err());
    let error = result.unwrap_err();
    match error {
        EvaluatePermissionsError::EntityResolutionError(msg) => {
            assert_eq!(msg, "Mock failure");
        }
        _ => panic!("Expected entity resolution error"),
    }
}
