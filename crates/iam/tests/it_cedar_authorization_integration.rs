#![cfg(feature = "integration-cedar")]

use std::sync::Arc;
use std::time::Duration;
use iam::{
    application::api::IamApi,
    infrastructure::{cedar_authorizer::CedarAuthorizer, cedar_policy_validator::CedarPolicyValidator, mongo_policy_repository::MongoPolicyRepository, mongo_user_repository::MongoUserRepository},
    features::{
        create_policy::CreatePolicyCommand,
        create_user::CreateUserCommand, 
        attach_policy_to_user::AttachPolicyToUserCommand,
        get_policy::GetPolicyQuery,
        get_user::GetUserQuery,
        authorize::AuthorizeQuery,
    },
    domain::policy::{Policy, PolicyStatus},
    domain::user::{User, UserStatus},
};
use async_trait::async_trait;
use infra_mongo::test_util::mongo_test_container::ephemeral_store;
use shared::UserId;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::redis::Redis;
use testcontainers::clients;
use tokio::time::{sleep, timeout};
use cedar_policy::{PolicySet, Context, Entities};
use serde_json::json;

async fn setup_dependencies() -> (MongoUserRepository, MongoPolicyRepository, CedarAuthorizer<'static>) {
    let (factory, _container) = ephemeral_store().await.unwrap();
    let client = factory.client().await.unwrap();
    
    let user_collection = client.database("iam_test").collection::<User>("users");
    let user_repo = MongoUserRepository::new(user_collection);
    
    let policy_collection = client.database("iam_test").collection::<Policy>("policies");
    let policy_repo = MongoPolicyRepository::new(policy_collection);
    
    // Redis para cache de autorización
    let docker = clients::Cli::default();
    let redis_container = docker.run(Redis::default());
    let redis_url = format!("redis://localhost:{}", redis_container.get_host_port_ipv4(6379));
    let authorizer = CedarAuthorizer::new(PolicySet::new(), &redis_url).await.unwrap();
    
    (user_repo, policy_repo, authorizer)
}

fn create_test_user_command() -> CreateUserCommand {
    CreateUserCommand {
        username: "testuser".to_string(),
        email: "test@example.com".to_string(),
        password: "password123".to_string(),
        attributes: serde_json::json!({ "department": "engineering", "role": "developer" }),
    }
}

fn create_read_policy_command() -> CreatePolicyCommand {
    CreatePolicyCommand {
        name: "read-artifacts".to_string(),
        description: Some("Allow reading artifacts".to_string()),
        content: r#"
permit(
    principal == User::"testuser",
    action == Action::"read",
    resource in Resource::"artifact"
);
"#.to_string(),
    }
}

fn create_write_policy_command() -> CreatePolicyCommand {
    CreatePolicyCommand {
        name: "write-artifacts".to_string(),
        description: Some("Allow writing artifacts".to_string()),
        content: r#"
permit(
    principal == User::"testuser",
    action == Action::"write", 
    resource in Resource::"artifact"
) when {
    principal.department == "engineering"
};
"#.to_string(),
    }
}

fn create_admin_policy_command() -> CreatePolicyCommand {
    CreatePolicyCommand {
        name: "admin-access".to_string(),
        description: Some("Full admin access".to_string()),
        content: r#"
permit(
    principal,
    action,
    resource
) when {
    principal.role == "admin"
};
"#.to_string(),
    }
}

#[tokio::test]
async fn test_cedar_authorization_with_user_attributes() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    // Create user and policy
    let user_id = api.create_user(create_test_user_command()).await.unwrap();
    let policy_id = api.create_policy(create_write_policy_command()).await.unwrap();
    
    // Attach policy to user
    let attach_cmd = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: policy_id.clone(),
    };
    api.attach_policy_to_user(attach_cmd).await.unwrap();
    
    // Wait for policy to be active
    sleep(Duration::from_millis(100)).await;
    
    // Act - Test authorization
    let authorize_query = AuthorizeQuery {
        principal: format!("User::\"{}\"", user_id.to_string()),
        action: "write".to_string(),
        resource: "Artifact::\"test-artifact\"".to_string(),
        context: Some(serde_json::json!({})),
    };
    
    let result = api.authorize(authorize_query).await;
    
    // Assert - Should be authorized (user has engineering department)
    assert!(result.is_ok());
    assert!(result.unwrap().authorized);
}

#[tokio::test]
async fn test_cedar_authorization_denied_without_attributes() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    // Create user without engineering department
    let mut user_cmd = create_test_user_command();
    user_cmd.attributes = serde_json::json!({ "department": "marketing", "role": "analyst" });
    
    let user_id = api.create_user(user_cmd).await.unwrap();
    let policy_id = api.create_policy(create_write_policy_command()).await.unwrap();
    
    // Attach policy to user
    let attach_cmd = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: policy_id.clone(),
    };
    api.attach_policy_to_user(attach_cmd).await.unwrap();
    
    sleep(Duration::from_millis(100)).await;
    
    // Act - Test authorization
    let authorize_query = AuthorizeQuery {
        principal: format!("User::\"{}\"", user_id.to_string()),
        action: "write".to_string(),
        resource: "Artifact::\"test-artifact\"".to_string(),
        context: Some(serde_json::json!({})),
    };
    
    let result = api.authorize(authorize_query).await;
    
    // Assert - Should be denied (user doesn't have engineering department)
    assert!(result.is_ok());
    assert!(!result.unwrap().authorized);
}

#[tokio::test]
async fn test_cedar_authorization_with_context_variables() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    // Create time-based policy
    let time_policy = CreatePolicyCommand {
        name: "business-hours-access".to_string(),
        description: Some("Access only during business hours".to_string()),
        content: r#"
permit(
    principal,
    action == Action::"read",
    resource
) when {
    context.time.hour >= 9 && context.time.hour < 17
};
"#.to_string(),
    };
    
    let policy_id = api.create_policy(time_policy).await.unwrap();
    let user_id = api.create_user(create_test_user_command()).await.unwrap();
    
    let attach_cmd = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: policy_id.clone(),
    };
    api.attach_policy_to_user(attach_cmd).await.unwrap();
    
    sleep(Duration::from_millis(100)).await;
    
    // Act - Test with different times
    let business_hours_context = serde_json::json!({
        "time": {
            "hour": 14,  // 2 PM - business hours
            "minute": 30
        }
    });
    
    let after_hours_context = serde_json::json!({
        "time": {
            "hour": 20,  // 8 PM - after hours
            "minute": 0
        }
    });
    
    let business_query = AuthorizeQuery {
        principal: format!("User::\"{}\"", user_id.to_string()),
        action: "read".to_string(),
        resource: "Artifact::\"test-artifact\"".to_string(),
        context: Some(business_hours_context),
    };
    
    let after_hours_query = AuthorizeQuery {
        principal: format!("User::\"{}\"", user_id.to_string()),
        action: "read".to_string(),
        resource: "Artifact::\"test-artifact\"".to_string(),
        context: Some(after_hours_context),
    };
    
    let business_result = api.authorize(business_query).await.unwrap();
    let after_hours_result = api.authorize(after_hours_query).await.unwrap();
    
    // Assert - Different results based on context
    assert!(business_result.authorized);  // Should be allowed during business hours
    assert!(!after_hours_result.authorized); // Should be denied after hours
}

#[tokio::test]
async fn test_cedar_policy_validation() {
    // Arrange
    let (user_repo, policy_repo, _) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    // Test valid policy
    let valid_policy = create_read_policy_command();
    let validation_result = api.create_policy(valid_policy).await;
    assert!(validation_result.is_ok());
    
    // Test invalid policy syntax
    let invalid_policy = CreatePolicyCommand {
        name: "invalid-policy".to_string(),
        description: Some("Invalid policy syntax".to_string()),
        content: r#"permit(principal, action, resource"#.to_string(), // Missing closing paren
    };
    
    let invalid_result = api.create_policy(invalid_policy).await;
    assert!(invalid_result.is_err());
    assert!(invalid_result.unwrap_err().to_string().contains("validation"));
}

#[tokio::test]
async fn test_cedar_policy_evaluation_order() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    // Create multiple policies for the same user
    let user_id = api.create_user(create_test_user_command()).await.unwrap();
    
    let read_policy_id = api.create_policy(create_read_policy_command()).await.unwrap();
    let write_policy_id = api.create_policy(create_write_policy_command()).await.unwrap();
    
    // Attach both policies
    let attach_read = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: read_policy_id.clone(),
    };
    api.attach_policy_to_user(attach_read).await.unwrap();
    
    let attach_write = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: write_policy_id.clone(),
    };
    api.attach_policy_to_user(attach_write).await.unwrap();
    
    sleep(Duration::from_millis(100)).await;
    
    // Act - Test different actions
    let read_query = AuthorizeQuery {
        principal: format!("User::\"{}\"", user_id.to_string()),
        action: "read".to_string(),
        resource: "Artifact::\"test-artifact\"".to_string(),
        context: Some(serde_json::json!({})),
    };
    
    let write_query = AuthorizeQuery {
        principal: format!("User::\"{}\"", user_id.to_string()),
        action: "write".to_string(),
        resource: "Artifact::\"test-artifact\"".to_string(),
        context: Some(serde_json::json!({})),
    };
    
    let delete_query = AuthorizeQuery {
        principal: format!("User::\"{}\"", user_id.to_string()),
        action: "delete".to_string(),
        resource: "Artifact::\"test-artifact\"".to_string(),
        context: Some(serde_json::json!({})),
    };
    
    let read_result = api.authorize(read_query).await.unwrap();
    let write_result = api.authorize(write_query).await.unwrap();
    let delete_result = api.authorize(delete_query).await.unwrap();
    
    // Assert - Different permissions for different actions
    assert!(read_result.authorized);   // Read should be allowed
    assert!(write_result.authorized);  // Write should be allowed (user has engineering department)
    assert!(!delete_result.authorized); // Delete should be denied (no policy for delete)
}

#[tokio::test]
async fn test_cedar_admin_policy_with_role() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    // Create admin user
    let mut admin_user_cmd = create_test_user_command();
    admin_user_cmd.attributes = serde_json::json!({ "department": "it", "role": "admin" });
    
    let admin_user_id = api.create_user(admin_user_cmd).await.unwrap();
    
    // Create admin policy
    let admin_policy_id = api.create_policy(create_admin_policy_command()).await.unwrap();
    
    let attach_cmd = AttachPolicyToUserCommand {
        user_id: admin_user_id.clone(),
        policy_id: admin_policy_id.clone(),
    };
    api.attach_policy_to_user(attach_cmd).await.unwrap();
    
    sleep(Duration::from_millis(100)).await;
    
    // Act - Test admin access to various resources and actions
    let test_cases = vec![
        ("read", "Artifact::\"test\""),
        ("write", "Artifact::\"test\""),
        ("delete", "Artifact::\"test\""),
        ("manage", "User::\"other\""),
        ("configure", "System::\"settings\""),
    ];
    
    for (action, resource) in test_cases {
        let query = AuthorizeQuery {
            principal: format!("User::\"{}\"", admin_user_id.to_string()),
            action: action.to_string(),
            resource: resource.to_string(),
            context: Some(serde_json::json!({})),
        };
        
        let result = api.authorize(query).await.unwrap();
        assert!(result.authorized, "Admin should have access to {} on {}", action, resource);
    }
}

#[tokio::test]
async fn test_cedar_policy_conflict_resolution() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    let user_id = api.create_user(create_test_user_command()).await.unwrap();
    
    // Create allow and deny policies for the same action
    let allow_policy = CreatePolicyCommand {
        name: "allow-read".to_string(),
        description: Some("Allow reading".to_string()),
        content: r#"
permit(
    principal == User::"testuser",
    action == Action::"read",
    resource == Artifact::"conflict-test"
);
"#.to_string(),
    };
    
    let deny_policy = CreatePolicyCommand {
        name: "deny-read".to_string(),
        description: Some("Deny reading".to_string()),
        content: r#"
forbid(
    principal == User::"testuser", 
    action == Action::"read",
    resource == Artifact::"conflict-test"
);
"#.to_string(),
    };
    
    let allow_policy_id = api.create_policy(allow_policy).await.unwrap();
    let deny_policy_id = api.create_policy(deny_policy).await.unwrap();
    
    // Attach both policies
    let attach_allow = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: allow_policy_id.clone(),
    };
    api.attach_policy_to_user(attach_allow).await.unwrap();
    
    let attach_deny = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: deny_policy_id.clone(),
    };
    api.attach_policy_to_user(attach_deny).await.unwrap();
    
    sleep(Duration::from_millis(100)).await;
    
    // Act - Test authorization with conflicting policies
    let query = AuthorizeQuery {
        principal: format!("User::\"{}\"", user_id.to_string()),
        action: "read".to_string(),
        resource: "Artifact::\"conflict-test\"".to_string(),
        context: Some(serde_json::json!({})),
    };
    
    let result = api.authorize(query).await.unwrap();
    
    // Assert - Deny should take precedence over permit
    assert!(!result.authorized, "Deny policy should take precedence over permit policy");
}

#[tokio::test]
async fn test_cedar_authorization_performance() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    let user_id = api.create_user(create_test_user_command()).await.unwrap();
    let policy_id = api.create_policy(create_read_policy_command()).await.unwrap();
    
    let attach_cmd = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: policy_id.clone(),
    };
    api.attach_policy_to_user(attach_cmd).await.unwrap();
    
    sleep(Duration::from_millis(100)).await;
    
    // Act - Measure authorization performance
    let query = AuthorizeQuery {
        principal: format!("User::\"{}\"", user_id.to_string()),
        action: "read".to_string(),
        resource: "Artifact::\"test\"".to_string(),
        context: Some(serde_json::json!({})),
    };
    
    let start = std::time::Instant::now();
    
    // Multiple authorization requests to test performance
    for _ in 0..10 {
        let result = api.authorize(query.clone()).await;
        assert!(result.is_ok());
    }
    
    let duration = start.elapsed();
    
    // Assert - Should complete within reasonable time
    assert!(duration < Duration::from_millis(500), "Authorization took too long: {:?}", duration);
}

#[tokio::test]
async fn test_cedar_authorization_with_complex_conditions() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    // Create policy with complex conditions
    let complex_policy = CreatePolicyCommand {
        name: "complex-access".to_string(),
        description: Some("Complex access conditions".to_string()),
        content: r#"
permit(
    principal,
    action == Action::"access",
    resource == Artifact::"sensitive"
) when {
    principal.department == "security" &&
    context.authentication.method == "mfa" &&
    context.authentication.strength >= 2 &&
    context.request.ip in ip("10.0.0.0/8") &&
    context.time.hour >= 9 && context.time.hour < 17
};
"#.to_string(),
    };
    
    let policy_id = api.create_policy(complex_policy).await.unwrap();
    
    // Create security user
    let mut security_user_cmd = create_test_user_command();
    security_user_cmd.username = "securityuser".to_string();
    security_user_cmd.attributes = serde_json::json!({ "department": "security", "role": "analyst" });
    
    let user_id = api.create_user(security_user_cmd).await.unwrap();
    
    let attach_cmd = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: policy_id.clone(),
    };
    api.attach_policy_to_user(attach_cmd).await.unwrap();
    
    sleep(Duration::from_millis(100)).await;
    
    // Test cases
    let valid_context = serde_json::json!({
        "authentication": {
            "method": "mfa",
            "strength": 2
        },
        "request": {
            "ip": "10.1.2.3"
        },
        "time": {
            "hour": 14,
            "minute": 30
        }
    });
    
    let invalid_context = serde_json::json!({
        "authentication": {
            "method": "password", // Wrong method
            "strength": 2
        },
        "request": {
            "ip": "192.168.1.1" // Wrong IP range
        },
        "time": {
            "hour": 20, // After hours
            "minute": 0
        }
    });
    
    let valid_query = AuthorizeQuery {
        principal: format!("User::\"{}\"", user_id.to_string()),
        action: "access".to_string(),
        resource: "Artifact::\"sensitive\"".to_string(),
        context: Some(valid_context),
    };
    
    let invalid_query = AuthorizeQuery {
        principal: format!("User::\"{}\"", user_id.to_string()),
        action: "access".to_string(),
        resource: "Artifact::\"sensitive\"".to_string(),
        context: Some(invalid_context),
    };
    
    let valid_result = api.authorize(valid_query).await.unwrap();
    let invalid_result = api.authorize(invalid_query).await.unwrap();
    
    // Assert
    assert!(valid_result.authorized, "Should be authorized with valid context");
    assert!(!invalid_result.authorized, "Should be denied with invalid context");
}