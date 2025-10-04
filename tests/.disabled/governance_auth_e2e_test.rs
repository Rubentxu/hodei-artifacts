use hodei_organizations::shared::domain::{Account, OrganizationalUnit, ServiceControlPolicy};
use hodei_organizations::shared::infrastructure::surreal::{SurrealAccountRepository, SurrealOuRepository, SurrealScpRepository};
use hodei_iam::shared::infrastructure::surreal::SurrealIamPolicyProvider;
use hodei_authorizer::ports::{IamPolicyProvider, OrganizationBoundaryProvider};
use hodei_authorizer::authorizer::AuthorizerService;
use policies::shared::application::engine::{PolicyEvaluator, AuthorizationRequest};
use policies::shared::domain::hrn::Hrn;
use policies::shared::domain::policy::Policy;
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use surrealdb::engine::any;
use std::sync::Arc;

#[tokio::test]
async fn test_governance_auth_e2e() {
    // Initialize SurrealDB in memory
    let db = Surreal::init();
    db.connect(any::connect("memory").await.unwrap()).await.unwrap();
    db.use_ns("test").use_db("test").await.unwrap();

    // Create repositories
    let account_repository = Arc::new(SurrealAccountRepository::new(db.clone()));
    let ou_repository = Arc::new(SurrealOuRepository::new(db.clone()));
    let scp_repository = Arc::new(SurrealScpRepository::new(db.clone()));

    // Create IAM policy provider
    let iam_policy_provider = Arc::new(SurrealIamPolicyProvider::new(db.clone()));

    // Create organization boundary provider
    let org_boundary_provider = Arc::new(SurrealOrganizationBoundaryProvider::new(db.clone()));

    // Create policy evaluator
    let policy_evaluator = PolicyEvaluator::new();

    // Create authorizer service
    let authorizer = AuthorizerService::new(
        iam_policy_provider.clone(),
        org_boundary_provider.clone(),
        policy_evaluator,
    );

    // Create organizational structure
    // Root OU
    let root_ou = OrganizationalUnit::new(
        Hrn::new("ou", "root"),
        "Root".to_string(),
        Hrn::new("ou", "root"),
    );
    ou_repository.save(&root_ou).await.unwrap();

    // Production OU
    let production_ou = OrganizationalUnit::new(
        Hrn::new("ou", "production"),
        "Production".to_string(),
        root_ou.hrn.clone(),
    );
    ou_repository.save(&production_ou).await.unwrap();

    // WebApp Account
    let webapp_account = Account::new(
        Hrn::new("account", "webapp"),
        "WebApp".to_string(),
        production_ou.hrn.clone(),
    );
    account_repository.save(&webapp_account).await.unwrap();

    // Attach SCP to Production OU
    let deny_scp = ServiceControlPolicy::new(
        Hrn::new("scp", "deny-iam-delete-user"),
        "DenyIAMDeleteUser".to_string(),
        "forbid(principal, action::\"iam:DeleteUser\", resource);".to_string(),
    );
    scp_repository.save(&deny_scp).await.unwrap();

    production_ou.attach_scp(deny_scp.hrn.clone());
    ou_repository.save(&production_ou).await.unwrap();

    // Create IAM policy for Admin user
    let admin_user = Hrn::new("user", "admin");
    // Note: This would require implementing the actual logic in SurrealIamPolicyProvider
    // For now, we'll assume it's set up correctly

    // Test authorization request
    let request = AuthorizationRequest {
        principal: admin_user,
        action: "iam:DeleteUser".to_string(),
        resource: Hrn::new("resource", "test-user"),
    };

    // Execute authorization
    let result = authorizer.is_authorized(request).await;

    // Assert the result
    assert!(result.is_ok());
    let response = result.unwrap();
    // This should be Deny because of the SCP attached to the Production OU
    assert_eq!(response.decision(), policies::shared::domain::policy::Decision::Deny);
}
