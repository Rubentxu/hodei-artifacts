use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;
use std::sync::Arc;

use artifact::infrastructure::{
    KafkaArtifactEventPublisher, MongoArtifactRepository, S3ArtifactStorage,
};
use aws_config::BehaviorVersion;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::Client as S3Client;
use cedar_policy::PolicySet;
use iam::application::ports::DecisionCache;
use iam::infrastructure::cedar_authorizer::CedarAuthorizer;

use infra_mongo::{MongoClientFactory, MongoConfig};
use mongodb::Client as MongoClient;
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::str::FromStr;

// No-op DecisionCache for testing purposes
pub struct NoopDecisionCache;

#[async_trait::async_trait]
impl DecisionCache for NoopDecisionCache {
    async fn get(
        &self,
        _key: &str,
    ) -> Result<Option<(cedar_policy::Decision, std::collections::HashSet<cedar_policy::PolicyId>)>, iam::error::IamError>
    {
        Ok(None)
    }
    async fn set(
        &self,
        _key: &str,
        _decision: cedar_policy::Decision,
        _reason: std::collections::HashSet<cedar_policy::PolicyId>,
        _ttl_seconds: usize,
    ) -> Result<(), iam::error::IamError> {
        Ok(())
    }
}

static CACHE: NoopDecisionCache = NoopDecisionCache;

// Function to start Docker Compose
pub fn start_docker_compose(compose_file_path: &str) -> Result<(), String> {
    println!("Starting Docker Compose environment from: {}", compose_file_path);

    // Extract project name from file path (remove .yml extension and path)
    let file_name = std::path::Path::new(compose_file_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .trim_end_matches(".yml");

    let output = Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(compose_file_path)
        .arg("-p")
        .arg(file_name) // Use unique project name
        .arg("up")
        .arg("-d")
        .output()
        .map_err(|e| format!("Failed to execute docker compose up: {}", e))?;

    if !output.status.success() {
        return Err(format!("Error starting Docker Compose: {}", String::from_utf8_lossy(&output.stderr)));
    }
    println!("Docker Compose environment started.");
    Ok(())
}

// Function to tear down Docker Compose
pub fn teardown_docker_compose(compose_file_path: &str) {
    println!("Tearing down Docker Compose environment from: {}", compose_file_path);
    
    // Extract project name from file path (remove .yml extension and path)
    let file_name = std::path::Path::new(compose_file_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .trim_end_matches(".yml");

    let output = Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(compose_file_path)
        .arg("-p")
        .arg(file_name) // Use unique project name
        .arg("down")
        .output()
        .expect("Failed to execute docker compose down");

    if !output.status.success() {
        eprintln!("Error tearing down Docker Compose: {}", String::from_utf8_lossy(&output.stderr));
    }
    println!("Docker Compose environment torn down.");
}

// Function to get mapped port
pub fn get_mapped_port(compose_file_path: &str, service_name: &str, container_port: &str) -> Result<u16, String> {
    // Extract project name from file path (remove .yml extension and path)
    let file_name = std::path::Path::new(compose_file_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .trim_end_matches(".yml");

    let port_output = Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(compose_file_path)
        .arg("-p")
        .arg(file_name) // Use unique project name
        .arg("port")
        .arg(service_name)
        .arg(container_port)
        .output()
        .map_err(|e| format!("Failed to get port for {}: {}", service_name, e))?;

    if !port_output.status.success() {
        return Err(format!("Error getting port for {}: {}", service_name, String::from_utf8_lossy(&port_output.stderr)));
    }

    let port_str = String::from_utf8_lossy(&port_output.stdout)
        .trim()
        .split(':')
        .last()
        .ok_or_else(|| format!("Failed to parse port for {}", service_name))?
        .to_string();

    port_str.parse::<u16>().map_err(|e| format!("Failed to parse port for {}: {}", service_name, e))
}

// Robust MongoDB health check
pub async fn wait_for_mongo_ready(compose_file_path: &str, mongo_port: u16) -> Result<MongoClientFactory, String> {
    println!("Waiting for MongoDB to be ready...");
    
    // Extract project name from file path (remove .yml extension and path)
    let file_name = std::path::Path::new(compose_file_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .trim_end_matches(".yml");

    let mut retries = 0;
    let max_retries = 60; // 60 * 1 second = 60 seconds timeout
    loop {
        let health_check_output = Command::new("docker")
            .arg("compose")
            .arg("-f")
            .arg(compose_file_path)
            .arg("-p")
            .arg(&file_name) // Use unique project name
            .arg("exec")
            .arg("mongodb")
            .arg("mongosh")
            .arg("--eval")
            .arg("db.adminCommand('ping').ok")
            .arg("--quiet")
            .output()
            .map_err(|e| format!("Failed to execute mongosh health check: {}", e))?;

        if health_check_output.status.success() && String::from_utf8_lossy(&health_check_output.stdout).trim() == "1" {
            let mongo_uri = format!("mongodb://127.0.0.1:{}", mongo_port);
            let mongo_config = MongoConfig {
                uri: mongo_uri.clone(),
                database: "hodei-test".to_string(),
                app_name: Some("hodei-test-runner".to_string()),
                max_pool_size: None,
                min_pool_size: None,
                tls: None,
            };
            let mongo_client_factory = MongoClientFactory::new(mongo_config);
            match mongo_client_factory.ping().await {
                Ok(_) => {
                    println!("MongoDB is ready and client connection established!");
                    return Ok(mongo_client_factory);
                },
                Err(e) => {
                    eprintln!("MongoDB client connection failed: {:?}", e);
                }
            }
        }

        retries += 1;
        if retries >= max_retries {
            return Err(format!("MongoDB health check failed after {} retries.", max_retries));
        }

        sleep(Duration::from_secs(1)).await;
    }
}

// Robust Kafka health check
pub async fn wait_for_kafka_ready(kafka_port: u16) -> Result<(), String> {
    println!("Waiting for Kafka to be ready...");
    let mut retries = 0;
    let max_retries = 60; // 60 * 1 second = 60 seconds timeout
    let kafka_brokers = format!("127.0.0.1:{}", kafka_port);
    loop {
        let producer: FutureProducer = rdkafka::ClientConfig::new()
            .set("bootstrap.servers", &kafka_brokers)
            .set("message.timeout.ms", "5000")
            .create()
            .map_err(|e| format!("Producer creation error: {}", e))?;

        let record = FutureRecord::to("test-topic")
            .payload("test-message")
            .key("test-key");

        match producer.send(record, Duration::from_secs(10)).await {
            Ok(_) => {
                println!("Kafka is ready!");
                return Ok(());
            },
            Err((e, _)) => {
                eprintln!("Kafka not ready yet: {:?}", e);
            }
        }

        retries += 1;
        if retries >= max_retries {
            return Err(format!("Kafka health check failed after {} retries.", max_retries));
        }

        sleep(Duration::from_secs(1)).await;
    }
}

// Robust S3 health check
pub async fn wait_for_s3_ready(s3_port: u16) -> Result<(), String> {
    println!("Waiting for LocalStack (S3) to be ready...");
    let mut retries = 0;
    let max_retries = 60; // 60 * 1 second = 60 seconds timeout
    let s3_endpoint_uri = format!("http://127.0.0.1:{}", s3_port);
    loop {
        let sdk_config = aws_config::defaults(BehaviorVersion::latest())
            .endpoint_url(&s3_endpoint_uri)
            .region(Region::new("us-east-1"))
            .credentials_provider(Credentials::new("test", "test", None, None, "test"))
            .load()
            .await;
        let s3_client = S3Client::new(&sdk_config);

        match s3_client.list_buckets().send().await {
            Ok(_) => {
                println!("LocalStack (S3) is ready!");
                return Ok(());
            },
            Err(e) => {
                eprintln!("LocalStack (S3) not ready yet: {:?}", e);
            }
        }

        retries += 1;
        if retries >= max_retries {
            return Err(format!("LocalStack (S3) health check failed after {} retries.", max_retries));
        }

        sleep(Duration::from_secs(1)).await;
    }
}

// Setup clients
pub async fn setup_mongo_client(factory: Arc<MongoClientFactory>) -> (MongoClient, Arc<MongoArtifactRepository>) {
    let mongo_client = factory.client().await.unwrap().clone();
    let artifact_repository = Arc::new(MongoArtifactRepository::new(factory));
    artifact_repository.ensure_indexes().await.unwrap();
    (mongo_client, artifact_repository)
}

pub async fn setup_s3_client(port: &u16) -> Arc<S3ArtifactStorage> {
    let s3_endpoint_uri = format!("http://127.0.0.1:{}", port);
    let sdk_config = aws_config::defaults(BehaviorVersion::latest())
        .endpoint_url(s3_endpoint_uri)
        .region(Region::new("us-east-1"))
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .load()
        .await;
    let s3_client = S3Client::new(&sdk_config);
    let bucket_name = "test-bucket".to_string();
    s3_client
        .create_bucket()
        .bucket(bucket_name.clone())
        .send()
        .await
        .unwrap();
    Arc::new(S3ArtifactStorage::new(s3_client, bucket_name))
}

pub fn setup_kafka_client(port: &u16) -> Arc<KafkaArtifactEventPublisher> {
    let kafka_brokers = format!("127.0.0.1:{}", port);
    Arc::new(KafkaArtifactEventPublisher::new(&kafka_brokers).unwrap())
}

pub async fn setup_authorization_client() -> Arc<CedarAuthorizer<'static>> {
    let policy_str = r#"
        permit(
            principal,
            action,
            resource
        );
    "#;
    let policies = PolicySet::from_str(policy_str).expect("Failed to parse Cedar policy");
    Arc::new(CedarAuthorizer::new(policies, &NoopDecisionCache))
}