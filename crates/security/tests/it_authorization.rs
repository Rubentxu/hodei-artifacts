// crates/security/tests/it_authorization.rs

use security::{
    AuthorizationService, AuthorizationRequest, AuthorizationDecision,
    Principal, Resource, Action, Context, AttributeValue
};
use security::infrastructure::cedar::service::CedarAuthorizationService;
use std::sync::Arc;

/// Integration tests for the complete authorization flow
/// These tests verify the end-to-end functionality from request to decision

#[tokio::test]
async fn test_alice_can_read_test_artifact() {
    // Arrange
    let service: Arc<dyn AuthorizationService> = Arc::new(
        CedarAuthorizationService::new().expect("Failed to create Cedar service")
    );
    
    let principal = Principal::new("alice".to_string(), "User".to_string());
    let action = Action::new("read".to_string());
    let resource = Resource::new("test-artifact".to_string(), "Artifact".to_string());
    let context = Context::new();
    
    let request = AuthorizationRequest::new(principal, action, resource, context);
    
    // Act
    let result = service.evaluate(request).await;
    
    // Assert
    assert!(result.is_ok(), "Authorization evaluation should succeed");
    let decision = result.unwrap();
    assert_eq!(decision, AuthorizationDecision::Allow, "Alice should be allowed to read test-artifact");
}

#[tokio::test]
async fn test_admin_has_full_access() {
    // Arrange
    let service: Arc<dyn AuthorizationService> = Arc::new(
        CedarAuthorizationService::new().expect("Failed to create Cedar service")
    );
    
    let principal = Principal::new("admin".to_string(), "User".to_string());
    let action = Action::new("delete".to_string());
    let resource = Resource::new("sensitive-data".to_string(), "Artifact".to_string());
    let context = Context::new();
    
    let request = AuthorizationRequest::new(principal, action, resource, context);
    
    // Act
    let result = service.evaluate(request).await;
    
    // Assert
    assert!(result.is_ok(), "Authorization evaluation should succeed");
    let decision = result.unwrap();
    assert_eq!(decision, AuthorizationDecision::Allow, "Admin should have full access to all resources");
}

#[tokio::test]
async fn test_unauthorized_user_denied() {
    // Arrange
    let service: Arc<dyn AuthorizationService> = Arc::new(
        CedarAuthorizationService::new().expect("Failed to create Cedar service")
    );
    
    let principal = Principal::new("bob".to_string(), "User".to_string());
    let action = Action::new("read".to_string());
    let resource = Resource::new("secret-document".to_string(), "Artifact".to_string());
    let context = Context::new();
    
    let request = AuthorizationRequest::new(principal, action, resource, context);
    
    // Act
    let result = service.evaluate(request).await;
    
    // Assert
    assert!(result.is_ok(), "Authorization evaluation should succeed");
    let decision = result.unwrap();
    assert_eq!(decision, AuthorizationDecision::Deny, "Bob should be denied access to unauthorized resources");
}

#[tokio::test]
async fn test_alice_denied_for_different_resource() {
    // Arrange
    let service: Arc<dyn AuthorizationService> = Arc::new(
        CedarAuthorizationService::new().expect("Failed to create Cedar service")
    );
    
    let principal = Principal::new("alice".to_string(), "User".to_string());
    let action = Action::new("read".to_string());
    let resource = Resource::new("other-artifact".to_string(), "Artifact".to_string());
    let context = Context::new();
    
    let request = AuthorizationRequest::new(principal, action, resource, context);
    
    // Act
    let result = service.evaluate(request).await;
    
    // Assert
    assert!(result.is_ok(), "Authorization evaluation should succeed");
    let decision = result.unwrap();
    assert_eq!(decision, AuthorizationDecision::Deny, "Alice should only have access to test-artifact");
}

#[tokio::test]
async fn test_alice_denied_for_write_action() {
    // Arrange
    let service: Arc<dyn AuthorizationService> = Arc::new(
        CedarAuthorizationService::new().expect("Failed to create Cedar service")
    );
    
    let principal = Principal::new("alice".to_string(), "User".to_string());
    let action = Action::new("write".to_string());
    let resource = Resource::new("test-artifact".to_string(), "Artifact".to_string());
    let context = Context::new();
    
    let request = AuthorizationRequest::new(principal, action, resource, context);
    
    // Act
    let result = service.evaluate(request).await;
    
    // Assert
    assert!(result.is_ok(), "Authorization evaluation should succeed");
    let decision = result.unwrap();
    assert_eq!(decision, AuthorizationDecision::Deny, "Alice should only be able to read, not write");
}

#[tokio::test]
async fn test_request_with_attributes() {
    // Arrange
    let service: Arc<dyn AuthorizationService> = Arc::new(
        CedarAuthorizationService::new().expect("Failed to create Cedar service")
    );
    
    let principal = Principal::new("alice".to_string(), "User".to_string())
        .with_attribute("department".to_string(), AttributeValue::String("engineering".to_string()))
        .with_attribute("clearance_level".to_string(), AttributeValue::Long(3));
    
    let action = Action::new("read".to_string())
        .with_attribute("scope".to_string(), AttributeValue::String("metadata".to_string()));
    
    let resource = Resource::new("test-artifact".to_string(), "Artifact".to_string())
        .with_attribute("classification".to_string(), AttributeValue::String("public".to_string()));
    
    let context = Context::new()
        .with_attribute("time_of_day".to_string(), AttributeValue::String("business_hours".to_string()))
        .with_attribute("ip_trusted".to_string(), AttributeValue::Boolean(true));
    
    let request = AuthorizationRequest::new(principal, action, resource, context);
    
    // Act
    let result = service.evaluate(request).await;
    
    // Assert
    assert!(result.is_ok(), "Authorization evaluation with attributes should succeed");
    let decision = result.unwrap();
    assert_eq!(decision, AuthorizationDecision::Allow, "Alice should still be allowed with attributes");
}

#[tokio::test]
async fn test_multiple_concurrent_requests() {
    // Arrange
    let service: Arc<dyn AuthorizationService> = Arc::new(
        CedarAuthorizationService::new().expect("Failed to create Cedar service")
    );
    
    // Create multiple requests
    let requests = vec![
        AuthorizationRequest::new(
            Principal::new("alice".to_string(), "User".to_string()),
            Action::new("read".to_string()),
            Resource::new("test-artifact".to_string(), "Artifact".to_string()),
            Context::new(),
        ),
        AuthorizationRequest::new(
            Principal::new("admin".to_string(), "User".to_string()),
            Action::new("write".to_string()),
            Resource::new("any-resource".to_string(), "Artifact".to_string()),
            Context::new(),
        ),
        AuthorizationRequest::new(
            Principal::new("bob".to_string(), "User".to_string()),
            Action::new("read".to_string()),
            Resource::new("secret".to_string(), "Artifact".to_string()),
            Context::new(),
        ),
    ];
    
    // Act - Execute requests concurrently
    let mut handles = vec![];
    for request in requests {
        let service_clone = Arc::clone(&service);
        let handle = tokio::spawn(async move {
            service_clone.evaluate(request).await
        });
        handles.push(handle);
    }
    
    // Collect results
    let mut results = vec![];
    for handle in handles {
        let result = handle.await.expect("Task should complete");
        results.push(result);
    }
    
    // Assert
    assert_eq!(results.len(), 3, "Should have 3 results");
    
    // Alice should be allowed
    assert!(results[0].is_ok());
    assert_eq!(results[0].as_ref().unwrap(), &AuthorizationDecision::Allow);
    
    // Admin should be allowed
    assert!(results[1].is_ok());
    assert_eq!(results[1].as_ref().unwrap(), &AuthorizationDecision::Allow);
    
    // Bob should be denied
    assert!(results[2].is_ok());
    assert_eq!(results[2].as_ref().unwrap(), &AuthorizationDecision::Deny);
}

#[tokio::test]
async fn test_error_handling_invalid_entity_format() {
    // Arrange
    let service: Arc<dyn AuthorizationService> = Arc::new(
        CedarAuthorizationService::new().expect("Failed to create Cedar service")
    );
    
    // Create request with invalid entity format (contains spaces)
    let principal = Principal::new("invalid user name".to_string(), "User".to_string());
    let action = Action::new("read".to_string());
    let resource = Resource::new("test-artifact".to_string(), "Artifact".to_string());
    let context = Context::new();
    
    let request = AuthorizationRequest::new(principal, action, resource, context);
    
    // Act
    let result = service.evaluate(request).await;
    
    // Assert
    // This should either succeed (if Cedar handles it) or fail gracefully
    // The important thing is that it doesn't panic
    match result {
        Ok(_) => {
            // Cedar handled the invalid format gracefully
            println!("Cedar handled invalid entity format gracefully");
        },
        Err(e) => {
            // Expected error for invalid format
            println!("Got expected error for invalid format: {}", e);
            assert!(e.to_string().contains("Invalid") || e.to_string().contains("Conversion"));
        }
    }
}