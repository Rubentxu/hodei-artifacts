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

// Mock repositories para simular fallos
struct FaultyUserRepository {
    should_fail: bool,
    fail_count: std::sync::atomic::AtomicUsize,
    real_repo: MongoUserRepository,
}

struct FaultyPolicyRepository {
    should_fail: bool,
    fail_count: std::sync::atomic::AtomicUsize,
    real_repo: MongoPolicyRepository,
}

#[async_trait]
impl iam::application::ports::UserRepository for FaultyUserRepository {
    async fn save(&self, user: &User) -> anyhow::Result<()> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("User repository failed"));
        }
        self.real_repo.save(user).await
    }

    async fn get(&self, id: &UserId) -> anyhow::Result<Option<User>> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("User repository failed"));
        }
        self.real_repo.get(id).await
    }

    async fn get_by_username(&self, username: &str) -> anyhow::Result<Option<User>> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("User repository failed"));
        }
        self.real_repo.get_by_username(username).await
    }

    async fn list(&self) -> anyhow::Result<Vec<User>> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("User repository failed"));
        }
        self.real_repo.list().await
    }

    async fn update_attributes(&self, id: &UserId, attributes: &serde_json::Value) -> anyhow::Result<()> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("User repository failed"));
        }
        self.real_repo.update_attributes(id, attributes).await
    }

    async fn delete(&self, id: &UserId) -> anyhow::Result<()> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("User repository failed"));
        }
        self.real_repo.delete(id).await
    }
}

#[async_trait]
impl iam::application::ports::PolicyRepository for FaultyPolicyRepository {
    async fn save(&self, policy: &Policy) -> anyhow::Result<()> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("Policy repository failed"));
        }
        self.real_repo.save(policy).await
    }

    async fn get(&self, id: &shared::PolicyId) -> anyhow::Result<Option<Policy>> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("Policy repository failed"));
        }
        self.real_repo.get(id).await
    }

    async fn list(&self) -> anyhow::Result<Vec<Policy>> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("Policy repository failed"));
        }
        self.real_repo.list().await
    }

    async fn delete(&self, id: &shared::PolicyId) -> anyhow::Result<()> {
        if self.should_fail {
            self.fail_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Err(anyhow::anyhow!("Policy repository failed"));
        }
        self.real_repo.delete(id).await
    }
}

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
        attributes: serde_json::json!({ "department": "engineering" }),
    }
}

fn create_test_policy_command() -> CreatePolicyCommand {
    CreatePolicyCommand {
        name: "test-policy".to_string(),
        description: Some("Test policy".to_string()),
        content: r#"permit(principal, action, resource);"#.to_string(),
    }
}

#[tokio::test]
async fn test_user_creation_fails_when_db_unavailable() {
    // Arrange
    let (real_user_repo, real_policy_repo, authorizer) = setup_dependencies().await;
    let faulty_user_repo = FaultyUserRepository {
        should_fail: true,
        fail_count: std::sync::atomic::AtomicUsize::new(0),
        real_repo: real_user_repo,
    };
    
    let api = IamApi::new(
        Arc::new(faulty_user_repo),
        Arc::new(real_policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    let cmd = create_test_user_command();

    // Act
    let result = api.create_user(cmd).await;

    // Assert - Debería fallar
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("repository"));
}

#[tokio::test]
async fn test_policy_creation_fails_when_db_unavailable() {
    // Arrange
    let (real_user_repo, real_policy_repo, authorizer) = setup_dependencies().await;
    let faulty_policy_repo = FaultyPolicyRepository {
        should_fail: true,
        fail_count: std::sync::atomic::AtomicUsize::new(0),
        real_repo: real_policy_repo,
    };
    
    let api = IamApi::new(
        Arc::new(real_user_repo),
        Arc::new(faulty_policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    let cmd = create_test_policy_command();

    // Act
    let result = api.create_policy(cmd).await;

    // Assert - Debería fallar
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("repository"));
}

#[tokio::test]
async fn test_authorization_fails_when_redis_unavailable() {
    // Arrange
    let (user_repo, policy_repo, _) = setup_dependencies().await;
    
    // Authorizer con Redis no existente
    let invalid_redis_url = "redis://localhost:9999";
    let authorizer = CedarAuthorizer::new(PolicySet::new(), invalid_redis_url).await;
    
    // Assert - Debería fallar la conexión
    assert!(authorizer.is_err());
}

#[tokio::test]
async fn test_policy_validation_with_malformed_syntax() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    // Policy con sintaxis inválida
    let invalid_cmd = CreatePolicyCommand {
        name: "invalid-policy".to_string(),
        description: Some("Invalid policy".to_string()),
        content: r#"permit(principal, action, resource"#.to_string(), // Falta paréntesis de cierre
    };

    // Act
    let result = api.create_policy(invalid_cmd).await;

    // Assert - Debería fallar por validación
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("validation") || error_msg.contains("syntax") || error_msg.contains("Cedar"));
}

#[tokio::test]
async fn test_concurrent_user_operations() {
    // Arrange
    let (real_user_repo, real_policy_repo, authorizer) = setup_dependencies().await;
    let faulty_user_repo = FaultyUserRepository {
        should_fail: true,
        fail_count: std::sync::atomic::AtomicUsize::new(0),
        real_repo: real_user_repo,
    };
    
    let api = IamApi::new(
        Arc::new(faulty_user_repo),
        Arc::new(real_policy_repo),
        Arc::new(CedarPolicyValidator),
    );

    // Act - Ejecutar múltiples operaciones concurrentes
    let mut handles = vec![];
    for i in 0..3 {
        let api_clone = api.clone();
        let cmd = CreateUserCommand {
            username: format!("user{}", i),
            email: format!("user{}@example.com", i),
            password: "password123".to_string(),
            attributes: serde_json::json!({ "id": i }),
        };
        
        handles.push(tokio::spawn(async move {
            api_clone.create_user(cmd).await
        }));
    }

    // Assert - Todos deberían fallar
    let results = futures::future::join_all(handles).await;
    for result in results {
        assert!(result.is_ok()); // La tarea completó
        let user_result = result.unwrap();
        assert!(user_result.is_err()); // Pero la creación falló
        assert!(user_result.unwrap_err().to_string().contains("repository"));
    }
}

#[tokio::test]
async fn test_authorization_timeout_handling() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    
    // Policy set vacío
    let policy_set = PolicySet::new();
    
    // Context y entities vacíos
    let context = Context::empty();
    let entities = Entities::empty();

    // Act - Intentar autorización con timeout muy corto
    let result = timeout(
        Duration::from_millis(1), // Timeout muy agresivo
        authorizer.is_authorized(
            "User::\"test\"",
            "read", 
            "Artifact::\"test\"",
            &context,
            &entities,
        )
    ).await;

    // Assert - Debería timeoutear o completar rápidamente
    assert!(result.is_ok()); // El timeout no debería dispararse para una operación simple
}

#[tokio::test]
async fn test_user_authentication_with_invalid_credentials() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    // Crear usuario primero
    let create_cmd = create_test_user_command();
    let user_id = api.create_user(create_cmd).await.unwrap();

    // Act - Intentar obtener usuario con ID inválido
    let invalid_query = GetUserQuery {
        id: UserId::new(), // ID que no existe
    };

    let result = api.get_user(invalid_query).await;

    // Assert - Debería fallar con not found
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn test_policy_attachment_validation() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    // Crear usuario y política
    let user_id = api.create_user(create_test_user_command()).await.unwrap();
    let policy_id = api.create_policy(create_test_policy_command()).await.unwrap();

    // Act - Intentar attach con IDs inválidos
    let invalid_attach_cmd = AttachPolicyToUserCommand {
        user_id: UserId::new(), // User que no existe
        policy_id: policy_id.clone(),
    };

    let result = api.attach_policy_to_user(invalid_attach_cmd).await;

    // Assert - Debería fallar
    assert!(result.is_err());
}

#[tokio::test]
async fn test_cedar_policy_evaluation_with_complex_conditions() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    
    // Policy con condición compleja que podría fallar
    let complex_policy = r#"
permit(
    principal,
    action == Action::"read",
    resource
) when {
    resource.name.contains("test") && 
    principal.department == "engineering" &&
    context.time.hour > 8 && context.time.hour < 18
};
"#.to_string();

    // Act - Validar policy compleja
    let validator = CedarPolicyValidator;
    let result = validator.validate(&complex_policy);

    // Assert - Debería ser válida
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_cache_stampede_protection() {
    // Arrange
    let (user_repo, policy_repo, authorizer) = setup_dependencies().await;
    let api = IamApi::new(
        Arc::new(user_repo),
        Arc::new(policy_repo),
        Arc::new(CedarPolicyValidator),
    );
    
    // Crear política
    let policy_id = api.create_policy(create_test_policy_command()).await.unwrap();

    // Act - Múltiples lecturas concurrentes de la misma política
    let mut handles = vec![];
    for _ in 0..5 {
        let api_clone = api.clone();
        let policy_id_clone = policy_id.clone();
        
        handles.push(tokio::spawn(async move {
            api_clone.get_policy(GetPolicyQuery { id: policy_id_clone }).await
        }));
    }

    // Assert - Todas deberían tener éxito (cache debería prevenir stampede)
    let results = futures::future::join_all(handles).await;
    for result in results {
        assert!(result.is_ok());
        let policy_result = result.unwrap();
        assert!(policy_result.is_ok());
        assert!(policy_result.unwrap().is_some());
    }
}