
use std::sync::Arc;

use artifact::infrastructure::{
    KafkaArtifactEventPublisher, S3ArtifactStorage,
};

use mongodb::Client as MongoClient;

mod compose_env;
pub use compose_env::ComposeTestEnvironment;

mod test_orchestrator;
pub mod dynamic_compose;
mod template_renderer;
mod resource_detector;
use dynamic_compose::{generate_unique_compose_file, DynamicPorts};



use artifact::infrastructure::MongoArtifactRepository;
use iam::infrastructure::cedar_authorizer::CedarAuthorizer;

use artifact::infrastructure::{
    RabbitMqArtifactEventPublisher, S3ArtifactStorage,
};

// ...

pub struct TestEnvironment {
    pub mongo_client: MongoClient,
    pub artifact_repository: Arc<MongoArtifactRepository>,
    pub artifact_storage: Arc<S3ArtifactStorage>,
    pub artifact_event_publisher: Arc<RabbitMqArtifactEventPublisher>,
    pub authorization: Arc<CedarAuthorizer<'static>>,
    actual_compose_file_path: String,
    is_generated_compose: bool,
    pub dynamic_ports: Option<DynamicPorts>,
}



impl Drop for TestEnvironment {
    fn drop(&mut self) {
        teardown_docker_compose(&self.actual_compose_file_path);
        
        // Clean up generated compose file if it was dynamically created
        if self.is_generated_compose {
            if let Err(e) = std::fs::remove_file(&self.actual_compose_file_path) {
                eprintln!("Warning: Failed to clean up generated compose file {}: {}", 
                         self.actual_compose_file_path, e);
            }
        }
    }
}



use crate::test_orchestrator::{
    start_docker_compose, teardown_docker_compose, get_mapped_port,
    wait_for_mongo_ready, wait_for_kafka_ready, wait_for_s3_ready,
    setup_mongo_client, setup_s3_client, setup_kafka_client, setup_authorization_client,
};

pub async fn setup_test_environment(compose_file_path_option: Option<&str>) -> TestEnvironment {
    let template_path = "/home/rubentxu/Proyectos/rust/hodei-artifacts/tests/compose/docker-compose.template.yml";
    
    // Generate a unique Docker Compose file to avoid network conflicts
    let (actual_compose_file_path, dynamic_ports, is_generated_compose) = if let Some(custom_path) = compose_file_path_option {
        (custom_path.to_string(), None, false)
    } else {
        // Generate unique compose file for parallel test execution using template
        let compose_result = generate_unique_compose_file(
            template_path, 
            "/tmp/hodei-test-compose"
        ).expect("Failed to generate unique Docker Compose file");
        (compose_result.file_path, Some(compose_result.ports), true)
    };

    start_docker_compose(&actual_compose_file_path)
        .expect("Failed to start Docker Compose environment");

    // Use dynamic ports if available, otherwise get mapped ports from Docker
    let (mongo_port, rabbitmq_port, s3_port) = if let Some(ports) = &dynamic_ports {
        (ports.mongo_port, ports.rabbitmq_port, ports.s3_port)
    } else {
        let mongo_port = get_mapped_port(&actual_compose_file_path, "mongodb", "27017")
            .expect("Failed to get MongoDB mapped port");
        let rabbitmq_port = get_mapped_port(&actual_compose_file_path, "rabbitmq", "5672")
            .expect("Failed to get RabbitMQ mapped port");
        let s3_port = get_mapped_port(&actual_compose_file_path, "localstack", "4566")
            .expect("Failed to get LocalStack mapped port");
        (mongo_port, rabbitmq_port, s3_port)
    };

    let mongo_client_factory = wait_for_mongo_ready(&actual_compose_file_path, mongo_port)
        .await
        .expect("MongoDB did not become ready");
    
    wait_for_rabbitmq_ready(rabbitmq_port)
        .await
        .expect("RabbitMQ did not become ready");

    wait_for_s3_ready(s3_port)
        .await
        .expect("LocalStack (S3) did not become ready");

    let (mongo_client, artifact_repository) = setup_mongo_client(Arc::new(mongo_client_factory)).await;
    let artifact_storage = setup_s3_client(&s3_port).await;
    let artifact_event_publisher = setup_rabbitmq_client(&rabbitmq_port).await.expect("Failed to setup RabbitMQ client");
    let authorization = setup_authorization_client().await;
    
    TestEnvironment {
        mongo_client,
        artifact_repository,
        artifact_storage,
        artifact_event_publisher,
        authorization,
        actual_compose_file_path,
        is_generated_compose,
        dynamic_ports,
    }
}

