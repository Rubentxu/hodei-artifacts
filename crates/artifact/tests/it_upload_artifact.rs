#![cfg(feature = "integration")]

// Pruebas de integración pesadas (Docker: MongoDB, MinIO, RabbitMQ) para upload_artifact.
// Ejecutar con: cargo test -p artifact --features integration -- --ignored

mod helpers;

#[cfg(test)]
mod tests {
    use crate::helpers;
    use axum::{
        Router,
        routing::post,
        Extension,
    };
    use reqwest::{
        multipart::{Form, Part},
        Client as ReqwestClient,
    };
    use serde_json::json;
    use testcontainers::core::{WaitFor, Healthcheck};
    use testcontainers::runners::AsyncRunner;
    use testcontainers::ContainerAsync;
    use testcontainers::Image;
    use testcontainers::core::ContainerPort;
    use testcontainers::GenericImage;
    use testcontainers::ImageExt;
    use tokio::net::TcpListener;
    use tracing::info;
    use std::sync::Arc;
    use std::time::Duration;

    use artifact::features::upload_artifact::{
        di::UploadArtifactDIContainer,
        dto::UploadArtifactResponse,
        api::UploadArtifactEndpoint,
    };
    use shared::models::PackageCoordinates;

    use mongodb::{Client as MongoClient, bson::{self, doc}};
    use aws_sdk_s3::{Client as S3Client, config::Region};
    use lapin::{Connection, ConnectionProperties, options::*, types::FieldTable, Channel};
    use futures_util::stream::TryStreamExt;
    use futures_util::StreamExt;

    // Custom Docker Image definitions
    #[derive(Debug, Default)]
    pub struct MongoDbImage;

    impl Image for MongoDbImage {
        fn name(&self) -> &str { "mongo" }
        fn tag(&self) -> &str { "5.0" } // Pinned to a known stable version
        fn ready_conditions(&self) -> Vec<WaitFor> {
            vec![WaitFor::message_on_stdout("waiting for connections on port")]
        }
        fn expose_ports(&self) -> &[ContainerPort] {
            &[ContainerPort::Tcp(27017)]
        }
    }

    #[derive(Debug, Default)]
    pub struct RabbitMqImage;

    impl Image for RabbitMqImage {
        fn name(&self) -> &str { "rabbitmq" }
        fn tag(&self) -> &str { "3.13-management" }
        fn ready_conditions(&self) -> Vec<WaitFor> {
            vec![WaitFor::message_on_stdout("Server startup complete")]
        }
        fn expose_ports(&self) -> &[ContainerPort] {
            &[ContainerPort::Tcp(5672)]
        }
    }

    struct TestContext {
        http_client: ReqwestClient,
        app_url: String,
        mongo_client: MongoClient,
        s3_client: S3Client,
        amqp_channel: Channel,
        _mongo_container: ContainerAsync<GenericImage>,
        _minio_container: ContainerAsync<GenericImage>,
        _rabbitmq_container: ContainerAsync<RabbitMqImage>,
    }

    async fn setup_test_environment_with_auth(disable_auth: bool) -> TestContext {
        helpers::setup_tracing();
        info!("Setting up test environment");
        
        // Set auth environment variable
        if disable_auth {
            unsafe { std::env::set_var("HODEI_AUTH_DISABLED", "true") };
        } else {
            unsafe { std::env::remove_var("HODEI_AUTH_DISABLED") };
        }

        // MongoDB
        let mongo_container = GenericImage::new("mongo", "5.0")
            .with_wait_for(WaitFor::Healthcheck(Default::default()))
            .with_health_check(Healthcheck::cmd(vec![
                "sh", "-c",
                "/usr/bin/mongo --eval 'db.adminCommand(\"ping\")' || exit 1"
            ])
            .with_retries(15)
            .with_interval(Duration::from_secs(3))
            .with_timeout(Duration::from_secs(10)))
            .with_startup_timeout(Duration::from_secs(180))
            .start().await.expect("Failed to start MongoDB container");
        let mongo_port = mongo_container.get_host_port_ipv4(27017).await.unwrap();
        let mongo_uri = format!("mongodb://127.0.0.1:{}", mongo_port);

        // MinIO
        let minio_image = GenericImage::new("minio/minio", "RELEASE.2023-03-20T20-16-18Z")
            .with_wait_for(WaitFor::message_on_stdout("API: http://"))
            .with_env_var("MINIO_ROOT_USER", "minioadmin")
            .with_env_var("MINIO_ROOT_PASSWORD", "minioadmin")
            .with_cmd(vec!["server", "/data"]);
        let minio_container = minio_image.start().await.expect("Failed to start MinIO container");
        let minio_port = minio_container.get_host_port_ipv4(9000).await.unwrap();
        let minio_endpoint = format!("http://127.0.0.1:{}", minio_port);

        let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .endpoint_url(minio_endpoint)
            .region(Region::new("us-east-1"))
            .credentials_provider(aws_credential_types::Credentials::new("minioadmin", "minioadmin", None, None, "static"))
            .load()
            .await;
        let s3_client = S3Client::new(&sdk_config);
        s3_client.create_bucket().bucket("test-bucket").send().await.ok(); // Ignore error if bucket exists

        // RabbitMQ
        let rabbitmq_container = RabbitMqImage::default().start().await.expect("Failed to start RabbitMQ container");
        let rabbitmq_port = rabbitmq_container.get_host_port_ipv4(5672).await.unwrap();
        let rabbit_uri = format!("amqp://guest:guest@127.0.0.1:{}", rabbitmq_port);
        let rabbit_conn = Connection::connect(&rabbit_uri, ConnectionProperties::default()).await.unwrap();
        let amqp_channel = rabbit_conn.create_channel().await.unwrap();

        // DI Container & App
        let di_container = UploadArtifactDIContainer::for_production(&sdk_config, &mongo_uri, "test_db", &rabbit_uri, "test_exchange", "test-bucket").await;
        let upload_artifact_api = di_container.endpoint;
        
        let app = Router::new()
            .route(
                "/artifacts",
                post(UploadArtifactEndpoint::upload_artifact),
            )
            .layer(Extension(upload_artifact_api));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service()).await.unwrap();
        });

        TestContext {
            http_client: ReqwestClient::new(),
            app_url,
            mongo_client: MongoClient::with_uri_str(&mongo_uri).await.unwrap(),
            s3_client,
            amqp_channel,
            _mongo_container: mongo_container,
            _minio_container: minio_container,
            _rabbitmq_container: rabbitmq_container,
        }
    }

    async fn setup_test_environment() -> TestContext {
        setup_test_environment_with_auth(true).await
    }

    #[tokio::test]
    async fn test_successful_upload() {
        let context = setup_test_environment().await;

        let coordinates = PackageCoordinates { namespace: Some("example".to_string()), name: "test-artifact".to_string(), version: "1.0.0".to_string(), qualifiers: Default::default() };
        let metadata = json!({ "coordinates": coordinates, "file_name": "test.bin" });
        let file_content = b"test content";
        let form = Form::new()
            .part("file", Part::bytes(file_content.as_ref()).file_name("test.bin"))
            .part("metadata", Part::text(metadata.to_string()));

        let response = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .header("X-Test-Bypass-Auth", "true")
            .multipart(form)
            .send().await.unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::CREATED);
        let body = response.json::<UploadArtifactResponse>().await.unwrap();
        assert!(body.hrn.contains("package-version/test-artifact/1.0.0"));
        assert!(body.hrn.contains("repository/default"));
    }

    #[tokio::test]
    async fn test_upload_with_invalid_checksum_should_fail() {
        let context = setup_test_environment().await;
        let metadata = json!({ "coordinates": { "namespace": "example", "name": "checksum-test", "version": "1.0", "qualifiers": {} }, "file_name": "test.bin", "checksum": "invalid-checksum" });
        let form = Form::new()
            .part("file", Part::bytes(b"content").file_name("test.bin"))
            .part("metadata", Part::text(metadata.to_string()));

        let response = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .header("X-Test-Bypass-Auth", "true")
            .multipart(form)
            .send().await.unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        let body_text = response.text().await.unwrap();
        assert_eq!(body_text, "Invalid checksum");
    }

    #[tokio::test]
    async fn test_upload_with_missing_file_part_should_fail() {
        let context = setup_test_environment().await;
        let metadata = json!({ "coordinates": { "namespace": "com.example", "name": "no-file-test", "version": "1.0", "qualifiers": {} } });
        let form = Form::new().part("metadata", Part::text(metadata.to_string()));

        let response = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .header("X-Test-Bypass-Auth", "true")
            .multipart(form)
            .send().await.unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_upload_with_missing_metadata_part_should_fail() {
        let context = setup_test_environment().await;
        let form = Form::new().part("file", Part::bytes(b"content").file_name("test.bin"));

        let response = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .header("X-Test-Bypass-Auth", "true")
            .multipart(form)
            .send().await.unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_upload_requires_auth_when_enabled() {
        let context = setup_test_environment_with_auth(false).await; // auth enabled
        let coordinates = PackageCoordinates { namespace: Some("example".to_string()), name: "test-artifact".to_string(), version: "1.0.0".to_string(), qualifiers: Default::default() };
        let metadata = json!({ "coordinates": coordinates, "file_name": "test.bin" });
        let form = Form::new()
            .part("file", Part::bytes(b"content").file_name("test.bin"))
            .part("metadata", Part::text(metadata.to_string()));
        let response = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .multipart(form)
            .send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_upload_with_bearer_token_succeeds() {
        let context = setup_test_environment_with_auth(false).await; // auth enabled
        let coordinates = PackageCoordinates { namespace: Some("example".to_string()), name: "auth-artifact".to_string(), version: "1.0.0".to_string(), qualifiers: Default::default() };
        let metadata = json!({ "coordinates": coordinates, "file_name": "auth.bin" });
        let form = Form::new()
            .part("file", Part::bytes(b"auth content").file_name("auth.bin"))
            .part("metadata", Part::text(metadata.to_string()));
        let response = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .header("Authorization", "Bearer testtoken")
            .multipart(form)
            .send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_duplicate_artifact_detection() {
        let context = setup_test_environment().await;

        // Setup RabbitMQ consumer BEFORE any uploads happen
        // First declare a queue and bind it to the exchange
        let queue = context.amqp_channel
            .queue_declare(
                "test-duplicate-queue",
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await
            .unwrap();

        context.amqp_channel
            .queue_bind(
                queue.name().as_str(),
                "test_exchange",
                "#", // Bind to all routing keys
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
            .unwrap();

        let mut consumer = context.amqp_channel
            .basic_consume(
                queue.name().as_str(),
                "test-consumer",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .unwrap();

        // First upload
        let coordinates = PackageCoordinates { namespace: Some("example".to_string()), name: "duplicate-test".to_string(), version: "1.0.0".to_string(), qualifiers: Default::default() };
        let metadata = json!({ "coordinates": coordinates, "file_name": "test.bin" });
        let file_content = b"identical content";
        let form = Form::new()
            .part("file", Part::bytes(file_content.as_ref()).file_name("test.bin"))
            .part("metadata", Part::text(metadata.to_string()));

        let response1 = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .header("X-Test-Bypass-Auth", "true")
            .multipart(form)
            .send().await.unwrap();

        assert_eq!(response1.status(), reqwest::StatusCode::CREATED);
        let body1 = response1.json::<UploadArtifactResponse>().await.unwrap();
        assert!(body1.hrn.contains("package-version/duplicate-test/1.0.0"));

        // Second upload with same content but different version
        let coordinates2 = PackageCoordinates { namespace: Some("example".to_string()), name: "duplicate-test".to_string(), version: "2.0.0".to_string(), qualifiers: Default::default() };
        let metadata2 = json!({ "coordinates": coordinates2, "file_name": "test.bin" });
        let form2 = Form::new()
            .part("file", Part::bytes(file_content.as_ref()).file_name("test.bin"))
            .part("metadata", Part::text(metadata2.to_string()));

        let response2 = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .header("X-Test-Bypass-Auth", "true")
            .multipart(form2)
            .send().await.unwrap();

        assert_eq!(response2.status(), reqwest::StatusCode::CREATED);
        let body2 = response2.json::<UploadArtifactResponse>().await.unwrap();
        assert!(body2.hrn.contains("package-version/duplicate-test/2.0.0"));

        // Verify that only one physical artifact exists in MongoDB
        let db = context.mongo_client.database("test_db");
        let collection: mongodb::Collection<bson::Document> = db.collection("physical_artifacts");
        let count = collection.count_documents(doc! {}).await.unwrap();
        assert_eq!(count, 1, "Should have only one physical artifact for identical content");

        // Verify that two package versions exist
        let pv_collection: mongodb::Collection<bson::Document> = db.collection("package_versions");
        let pv_count = pv_collection.count_documents(doc! {}).await.unwrap();
        assert_eq!(pv_count, 2, "Should have two package versions");

        // Verify RabbitMQ events - should have DuplicateArtifactDetected event

        // Wait for events with timeout
        let mut duplicate_event_found = false;
        let start_time = std::time::Instant::now();
        
        while start_time.elapsed() < Duration::from_secs(5) && !duplicate_event_found {
            if let Ok(Some(delivery)) = tokio::time::timeout(Duration::from_millis(100), consumer.next()).await {
                if let Ok(delivery) = delivery {
                    if let Ok(message) = std::str::from_utf8(&delivery.data) {
                        if message.contains("DuplicateArtifactDetected") {
                            duplicate_event_found = true;
                        }
                    }
                    delivery.ack(BasicAckOptions::default()).await.ok();
                }
            }
        }

        assert!(duplicate_event_found, "DuplicateArtifactDetected event should be published");
    }

    #[tokio::test]
    async fn test_different_content_creates_different_artifacts() {
        let context = setup_test_environment().await;

        // First upload
        let coordinates1 = PackageCoordinates { namespace: Some("example".to_string()), name: "different-test".to_string(), version: "1.0.0".to_string(), qualifiers: Default::default() };
        let metadata1 = json!({ "coordinates": coordinates1, "file_name": "test1.bin" });
        let file_content1 = b"content one";
        let form1 = Form::new()
            .part("file", Part::bytes(file_content1.as_ref()).file_name("test1.bin"))
            .part("metadata", Part::text(metadata1.to_string()));

        let response1 = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .header("X-Test-Bypass-Auth", "true")
            .multipart(form1)
            .send().await.unwrap();

        assert_eq!(response1.status(), reqwest::StatusCode::CREATED);

        // Second upload with different content
        let coordinates2 = PackageCoordinates { namespace: Some("example".to_string()), name: "different-test".to_string(), version: "2.0.0".to_string(), qualifiers: Default::default() };
        let metadata2 = json!({ "coordinates": coordinates2, "file_name": "test2.bin" });
        let file_content2 = b"content two";
        let form2 = Form::new()
            .part("file", Part::bytes(file_content2.as_ref()).file_name("test2.bin"))
            .part("metadata", Part::text(metadata2.to_string()));

        let response2 = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .header("X-Test-Bypass-Auth", "true")
            .multipart(form2)
            .send().await.unwrap();

        assert_eq!(response2.status(), reqwest::StatusCode::CREATED);

        // Verify that two physical artifacts exist in MongoDB
        let db = context.mongo_client.database("test_db");
        let collection: mongodb::Collection<bson::Document> = db.collection("physical_artifacts");
        let count = collection.count_documents(doc! {}).await.unwrap();
        assert_eq!(count, 2, "Should have two physical artifacts for different content");

        // Verify that two package versions exist
        let pv_collection: mongodb::Collection<bson::Document> = db.collection("package_versions");
        let pv_count = pv_collection.count_documents(doc! {}).await.unwrap();
        assert_eq!(pv_count, 2, "Should have two package versions");
    }
}

