#![cfg(feature = "integration-cedar")]

use shared_test::setup_test_environment;
use artifact::features::upload_artifact::{command::UploadArtifactCommand, handler::handle as handle_upload};
use iam::application::api::IamApi;
use iam::infrastructure::cedar_authorizer::CedarAuthorizer;
use iam::infrastructure::cedar_policy_validator::CedarPolicyValidator;
use iam::infrastructure::mongo_policy_repository::MongoPolicyRepository;
use iam::infrastructure::mongo_user_repository::MongoUserRepository;
use iam::features::create_policy::CreatePolicyCommand;
use iam::features::create_user::CreateUserCommand;
use iam::features::attach_policy_to_user::AttachPolicyToUserCommand;
use shared::{RepositoryId, UserId};
use std::sync::Arc;
use cedar_policy::{PolicySet, Context, Entities};

#[tokio::test]
async fn test_authorization_flow_with_cedar() -> Result<(), Box<dyn std::error::Error>> {
    let env = setup_test_environment(None).await;
    
    // Setup IAM API with real repositories
    let user_repo = Arc::new(MongoUserRepository::new(
        env.mongo_client.database("iam_test").collection("users")
    ));
    let policy_repo = Arc::new(MongoPolicyRepository::new(
        env.mongo_client.database("iam_test").collection("policies")
    ));
    let policy_validator = Arc::new(CedarPolicyValidator);
    
    let iam_api = IamApi::new(user_repo.clone(), policy_repo.clone(), policy_validator.clone());
    
    // Create test user
    let create_user_cmd = CreateUserCommand {
        username: "testuser".to_string(),
        email: "test@example.com".to_string(),
        password: "password123".to_string(),
        attributes: serde_json::json!({
            "department": "engineering",
            "role": "developer",
            "team": "backend"
        }),
    };
    
    let user_id = iam_api.create_user(create_user_cmd).await?;
    
    // Create authorization policy
    let create_policy_cmd = CreatePolicyCommand {
        name: "upload-artifact-policy".to_string(),
        description: Some("Allows developers to upload artifacts".to_string()),
        content: r#"
permit(
    principal in User::"engineering",
    action == Action::"upload",
    resource
);
"#.to_string(),
    };
    
    let policy_id = iam_api.create_policy(create_policy_cmd).await?;
    
    // Attach policy to user
    let attach_cmd = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: policy_id.clone(),
    };
    
    iam_api.attach_policy_to_user(attach_cmd).await?;
    
    // Create Cedar authorizer with the policy
    let mut policy_set = PolicySet::new();
    let policy_content = policy_repo.get(&policy_id).await?.unwrap().content;
    policy_set.add("upload-policy".parse()?, policy_content)?;
    
    let authorizer = CedarAuthorizer::new(policy_set, &env.redis_cache);
    
    // Test authorization decision
    let context = Context::empty();
    let entities = Entities::empty();
    
    // Should allow upload for engineering user
    let decision = authorizer.is_authorized(
        &format!("User::\"{}\"", user_id),
        "upload",
        "Artifact::\"test\"",
        &context,
        &entities,
    ).await?;
    
    assert!(decision, "User should be authorized to upload artifacts");
    
    // Test with different action - should deny
    let denied_decision = authorizer.is_authorized(
        &format!("User::\"{}\"", user_id),
        "delete", // Not allowed by policy
        "Artifact::\"test\"",
        &context,
        &entities,
    ).await?;
    
    assert!(!denied_decision, "User should not be authorized to delete artifacts");
    
    Ok(())
}

#[tokio::test]
async fn test_authorization_in_artifact_upload_flow() -> Result<(), Box<dyn std::error::Error>> {
    let env = setup_test_environment(None).await;
    
    // Setup IAM components
    let user_repo = Arc::new(MongoUserRepository::new(
        env.mongo_client.database("iam_test").collection("users")
    ));
    let policy_repo = Arc::new(MongoPolicyRepository::new(
        env.mongo_client.database("iam_test").collection("policies")
    ));
    let policy_validator = Arc::new(CedarPolicyValidator);
    
    let iam_api = IamApi::new(user_repo.clone(), policy_repo.clone(), policy_validator.clone());
    
    // Create user with specific attributes
    let create_user_cmd = CreateUserCommand {
        username: "uploader".to_string(),
        email: "uploader@example.com".to_string(),
        password: "password123".to_string(),
        attributes: serde_json::json!({
            "department": "operations",
            "permissions": ["read", "write"]
        }),
    };
    
    let user_id = iam_api.create_user(create_user_cmd).await?;
    
    // Create policy that allows upload for operations department
    let create_policy_cmd = CreatePolicyCommand {
        name: "ops-upload-policy".to_string(),
        description: Some("Allows operations team to upload artifacts".to_string()),
        content: r#"
permit(
    principal in User::"operations",
    action == Action::"upload",
    resource
);
"#.to_string(),
    };
    
    let policy_id = iam_api.create_policy(create_policy_cmd).await?;
    
    // Attach policy to user
    let attach_cmd = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: policy_id.clone(),
    };
    
    iam_api.attach_policy_to_user(attach_cmd).await?;
    
    // Test the actual upload flow with authorization
    let repository_id = RepositoryId::new();
    
    let upload_cmd = UploadArtifactCommand {
        repository_id: repository_id.clone(),
        version: artifact::domain::model::ArtifactVersion("1.0.0".to_string()),
        file_name: "authorized-upload.txt".to_string(),
        size_bytes: 15,
        checksum: artifact::domain::model::ArtifactChecksum("d41d8cd98f00b204e9800998ecf8427e".to_string()),
        user_id: user_id.clone(),
        bytes: b"authorized content".to_vec(),
    };
    
    // This should succeed because user has upload permission
    let result = handle_upload(
        &*env.artifact_repository,
        &*env.artifact_storage,
        &*env.artifact_event_publisher,
        upload_cmd,
    ).await;
    
    assert!(result.is_ok(), "Upload should succeed for authorized user");
    
    Ok(())
}

#[tokio::test]
async fn test_authorization_denied_flow() -> Result<(), Box<dyn std::error::Error>> {
    let env = setup_test_environment(None).await;
    
    // Setup IAM components
    let user_repo = Arc::new(MongoUserRepository::new(
        env.mongo_client.database("iam_test").collection("users")
    ));
    let policy_repo = Arc::new(MongoPolicyRepository::new(
        env.mongo_client.database("iam_test").collection("policies")
    ));
    let policy_validator = Arc::new(CedarPolicyValidator);
    
    let iam_api = IamApi::new(user_repo.clone(), policy_repo.clone(), policy_validator.clone());
    
    // Create user without upload permissions
    let create_user_cmd = CreateUserCommand {
        username: "viewer".to_string(),
        email: "viewer@example.com".to_string(),
        password: "password123".to_string(),
        attributes: serde_json::json!({
            "department": "marketing",
            "permissions": ["read"] // Only read permission, no upload
        }),
    };
    
    let user_id = iam_api.create_user(create_user_cmd).await?;
    
    // Create a restrictive policy that only allows read
    let create_policy_cmd = CreatePolicyCommand {
        name: "read-only-policy".to_string(),
        description: Some("Only allows read operations".to_string()),
        content: r#"
permit(
    principal,
    action == Action::"read",
    resource
);
"#.to_string(),
    };
    
    let policy_id = iam_api.create_policy(create_policy_cmd).await?;
    
    // Attach policy to user
    let attach_cmd = AttachPolicyToUserCommand {
        user_id: user_id.clone(),
        policy_id: policy_id.clone(),
    };
    
    iam_api.attach_policy_to_user(attach_cmd).await?;
    
    // Test upload attempt - should fail due to lack of permissions
    let repository_id = RepositoryId::new();
    
    let upload_cmd = UploadArtifactCommand {
        repository_id: repository_id.clone(),
        version: artifact::domain::model::ArtifactVersion("1.0.0".to_string()),
        file_name: "unauthorized-upload.txt".to_string(),
        size_bytes: 15,
        checksum: artifact::domain::model::ArtifactChecksum("d41d8cd98f00b204e9800998ecf8427e".to_string()),
        user_id: user_id.clone(),
        bytes: b"unauthorized content".to_vec(),
    };
    
    let result = handle_upload(
        &*env.artifact_repository,
        &*env.artifact_storage,
        &*env.artifact_event_publisher,
        upload_cmd,
    ).await;
    
    // Should fail with authorization error
    assert!(result.is_err(), "Upload should fail for unauthorized user");
    
    let err = result.unwrap_err();
    assert!(err.to_string().contains("authorization") || err.to_string().contains("permission"),
        "Error should indicate authorization failure: {:?}", err);
    
    Ok(())
}

#[tokio::test]
async fn test_cedar_policy_validation() -> Result<(), Box<dyn std::error::Error>> {
    let env = setup_test_environment(None).await;
    let policy_validator = CedarPolicyValidator;
    
    // Test valid policy
    let valid_policy = r#"
permit(
    principal == User::"test",
    action == Action::"read",
    resource == Resource::"artifact"
);
"#.to_string();
    
    let validation_result = policy_validator.validate(&valid_policy);
    assert!(validation_result.is_ok(), "Valid policy should pass validation");
    
    // Test invalid policy (syntax error)
    let invalid_policy = r#"
permit(
    principal == User::"test",
    action == Action::"read",
    resource == Resource::"artifact"
    // Missing closing parenthesis
"#.to_string();
    
    let validation_result = policy_validator.validate(&invalid_policy);
    assert!(validation_result.is_err(), "Invalid policy should fail validation");
    
    // Test policy with unsafe operations (should be rejected)
    let unsafe_policy = r#"
permit(
    principal,
    action,
    resource
) when { 1 / 0 == 0 }; // Division by zero
"#.to_string();
    
    let validation_result = policy_validator.validate(&unsafe_policy);
    assert!(validation_result.is_err(), "Unsafe policy should fail validation");
    
    Ok(())
}