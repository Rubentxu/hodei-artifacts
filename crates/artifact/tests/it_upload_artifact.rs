#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use axum::{
        Router,
        routing::post,
    };
    use reqwest::{
        multipart::{Form, Part},
        Client as ReqwestClient,
    };
    use serde_json::json;
    use testcontainers::{core::WaitFor, runners::AsyncRunner, ContainerAsync, Image, core::ContainerPort};
    use tokio::net::TcpListener;

    use artifact::features::upload_artifact::{
        di::UploadArtifactDIContainer,
        dto::UploadArtifactResponse,
    };
    use shared::models::PackageCoordinates;

    use mongodb::{Client as MongoClient, bson::doc};
    use aws_sdk_s3::{Client as S3Client, config::Region};
    use lapin::{Connection, ConnectionProperties};

    // Custom Docker Image definitions
    #[derive(Debug, Default)]
    pub struct MongoDbImage;

    impl Image for MongoDbImage {
        fn name(&self) -> &str {
            "mongo"
        }

        fn tag(&self) -> &str {
            "latest"
        }

        fn ready_conditions(&self) -> Vec<WaitFor> {
            vec![WaitFor::message_on_stdout("waiting for connections")]
        }

        fn expose_ports(&self) -> &[ContainerPort] {
            &[ContainerPort::Tcp(27017)]
        }
    }

    #[derive(Debug, Default)]
    pub struct MinioImage;

    impl Image for MinioImage {
        fn name(&self) -> &str {
            "minio/minio"
        }

        fn tag(&self) -> &str {
            "latest"
        }

        fn ready_conditions(&self) -> Vec<WaitFor> {
            vec![WaitFor::message_on_stdout("Server initialized")]
        }

        fn expose_ports(&self) -> &[ContainerPort] {
            &[ContainerPort::Tcp(9000), ContainerPort::Tcp(9001)]
        }

        fn entrypoint(&self) -> Option<&str> {
            Some("minio")
        }

        fn cmd(&self) -> Vec<String> {
            vec!["server".to_string(), "/data".to_string()]
        }
    }

    #[derive(Debug, Default)]
    pub struct RabbitMqImage;

    impl Image for RabbitMqImage {
        fn name(&self) -> &str {
            "rabbitmq"
        }

        fn tag(&self) -> &str {
            "management"
        }

        fn ready_conditions(&self) -> Vec<WaitFor> {
            vec![WaitFor::message_on_stdout("Server startup complete")]
        }

        fn expose_ports(&self) -> &[ContainerPort] {
            &[ContainerPort::Tcp(5672)]
        }

        fn env_vars(&self) -> Vec<(String, String)> {
            vec![
                ("RABBITMQ_DEFAULT_USER".to_string(), "guest".to_string()),
                ("RABBITMQ_DEFAULT_PASS".to_string(), "guest".to_string()),
            ]
        }
    }

    struct TestContext {
        http_client: ReqwestClient,
        app_url: String,
        mongo_client: MongoClient,
        s3_client: S3Client,
        rabbitmq_connection: lapin::Connection,
        _mongo_container: ContainerAsync<MongoDbImage>,
        _minio_container: ContainerAsync<MinioImage>,
        _rabbitmq_container: ContainerAsync<RabbitMqImage>,
    }

    async fn setup_test_environment() -> TestContext {
        // Start containers
        let mongo_container = MongoDbImage::default().start().await.unwrap();
        let minio_container = MinioImage::default().start().await.unwrap();
        let rabbitmq_container = RabbitMqImage::default().start().await.unwrap();

        let mongo_port = mongo_container.get_host_port_ipv4(27017).await.unwrap();
        let minio_port = minio_container.get_host_port_ipv4(9000).await.unwrap();
        let rabbitmq_port = rabbitmq_container.get_host_port_ipv4(5672).await.unwrap();

        let mongo_uri = &format!("mongodb://localhost:{}", mongo_port);
        let amqp_url = &format!("amqp://guest:guest@localhost:{}", rabbitmq_port);
        let s3_bucket = "test-bucket";

        // Configure S3
        let aws_config = aws_config::from_env()
            .endpoint_url(format!("http://localhost:{}", minio_port))
            .load()
            .await;

        // Create DI container with production dependencies
        let di_container = UploadArtifactDIContainer::for_production(
            &aws_config,
            mongo_uri,
            "test_db",
            amqp_url,
            "test_exchange",
            s3_bucket,
        ).await;

        // Create Axum app
        let app = Router::new().route("/artifacts", post({ 
            let endpoint = Arc::clone(&di_container.endpoint);
            move |multipart| async move {
                endpoint.handle_request(multipart).await
            }
        }));

        // Start server on a random port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Initialize direct clients for verification
        let mongo_client = MongoClient::with_uri_str(mongo_uri).await.unwrap();
        let s3_client = S3Client::new(&aws_config);
        let rabbitmq_connection = Connection::connect(amqp_url, ConnectionProperties::default()).await.unwrap();

        // Create S3 bucket
        s3_client.create_bucket().bucket(s3_bucket).send().await.unwrap();

        TestContext {
            http_client: ReqwestClient::new(),
            app_url,
            mongo_client,
            s3_client,
            rabbitmq_connection,
            _mongo_container: mongo_container,
            _minio_container: minio_container,
            _rabbitmq_container: rabbitmq_container,
        }
    }

    #[tokio::test]
    async fn test_upload_duplicate_package_version_integration() {
        // Arrange
        let context = setup_test_environment().await;

        let coordinates = PackageCoordinates {
            namespace: Some("com.my-org".to_string()),
            name: "my-app".to_string(),
            version: "1.2.3".to_string(),
            qualifiers: Default::default(),
        };
        let metadata = json!({
            "coordinates": coordinates,
            "file_name": "my-app-1.2.3.jar",
            "content_length": 27,
        });

        let file_content = b"This is a test file content.";
        let file_part = Part::bytes(file_content.as_ref()).file_name("my-app-1.2.3.jar");
        let metadata_part = Part::text(metadata.to_string());

        let form = Form::new()
            .part("file", file_part)
            .part("metadata", metadata_part);

        // First upload (should succeed)
        let response1 = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .multipart(Form::new()
                .part("file", Part::bytes(file_content.as_ref()).file_name("my-app-1.2.3.jar"))
                .part("metadata", Part::text(metadata.to_string())))
            .send()
            .await
            .unwrap();
        assert_eq!(response1.status(), reqwest::StatusCode::CREATED);

        // Second upload (should conflict)
        let response2 = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .multipart(Form::new()
                .part("file", Part::bytes(file_content.as_ref()).file_name("my-app-1.2.3.jar"))
                .part("metadata", Part::text(metadata.to_string())))
            .send()
            .await
            .unwrap();

        // Assert
        assert_eq!(response2.status(), reqwest::StatusCode::CONFLICT);

        // Verify MongoDB: counts should still be 1 for physical and 1 for package version
        let db = context.mongo_client.database("test_db");
        let physical_artifacts_collection: mongodb::Collection<mongodb::bson::Document> = db.collection("physical_artifacts");
        let package_versions_collection: mongodb::Collection<mongodb::bson::Document> = db.collection("package_versions");

        let physical_artifact_count = physical_artifacts_collection.count_documents(doc! {}).await.unwrap();
        assert_eq!(physical_artifact_count, 1);

        let package_version_count = package_versions_collection.count_documents(doc! {}).await.unwrap();
        assert_eq!(package_version_count, 1);

        // Verify RabbitMQ events (only one event should be published)
        let channel = context.rabbitmq_connection.create_channel().await.unwrap();
        let queue = channel.queue_declare("test_queue", lapin::options::QueueDeclareOptions::default(), lapin::types::FieldTable::default()).await.unwrap();
        channel.queue_bind(queue.name().as_str(), "test_exchange", "artifact.uploaded", lapin::options::QueueBindOptions::default(), lapin::types::FieldTable::default()).await.unwrap();
        
        let delivery = channel.basic_get(queue.name().as_str(), lapin::options::BasicGetOptions::default()).await.unwrap().unwrap();
        let payload = &delivery.data;
        let event_str = std::str::from_utf8(payload).unwrap();
        assert!(event_str.contains("PackageVersionPublished"));
        assert!(event_str.contains("package-version/com.my-org/my-app/1.2.3"));

        // Ensure no more messages are in RabbitMQ
        let no_message = channel.basic_get(queue.name().as_str(), lapin::options::BasicGetOptions::default()).await.unwrap();
        assert!(no_message.is_none());
    }
    
    #[tokio::test]
    async fn test_upload_artifact_integration() {
        // Arrange
        let context = setup_test_environment().await;

        let coordinates = PackageCoordinates {
            namespace: Some("com.my-org".to_string()),
            name: "my-app".to_string(),
            version: "1.2.3".to_string(),
            qualifiers: Default::default(),
        };
        let metadata = json!({
            "coordinates": coordinates,
            "file_name": "my-app-1.2.3.jar",
            "content_length": 27,
        });

        let file_content = b"This is a test file content.";
        let file_part = Part::bytes(file_content.as_ref()).file_name("my-app-1.2.3.jar");
        let metadata_part = Part::text(metadata.to_string());

        let form = Form::new()
            .part("file", file_part)
            .part("metadata", metadata_part);

        // Act
        let response = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .multipart(form)
            .send()
            .await
            .unwrap();

        // Assert
        assert_eq!(response.status(), reqwest::StatusCode::CREATED);
        let body = response.json::<UploadArtifactResponse>().await.unwrap();
        assert!(body.hrn.contains("package-version/com.my-org/my-app/1.2.3"));

        // Verify MongoDB
        let db = context.mongo_client.database("test_db");
        let physical_artifacts_collection: mongodb::Collection<mongodb::bson::Document> = db.collection("physical_artifacts");
        let package_versions_collection: mongodb::Collection<mongodb::bson::Document> = db.collection("package_versions");

        let physical_artifact_count = physical_artifacts_collection.count_documents(doc! {}).await.unwrap();
        assert_eq!(physical_artifact_count, 1);

        let package_version_count = package_versions_collection.count_documents(doc! {}).await.unwrap();
        assert_eq!(package_version_count, 1);

        // Verify MinIO
        let s3_object = context.s3_client
            .get_object()
            .bucket("test-bucket")
            .key("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855") // SHA256 of "This is a test file content."
            .send()
            .await
            .unwrap();
        let bytes = s3_object.body.collect().await.unwrap().into_bytes();
        assert_eq!(bytes.as_ref(), file_content);

        // Verify RabbitMQ event
        let channel = context.rabbitmq_connection.create_channel().await.unwrap();
        let queue = channel.queue_declare("test_queue", lapin::options::QueueDeclareOptions::default(), lapin::types::FieldTable::default()).await.unwrap();
        channel.queue_bind(queue.name().as_str(), "test_exchange", "artifact.uploaded", lapin::options::QueueBindOptions::default(), lapin::types::FieldTable::default()).await.unwrap();
        
        let delivery = channel.basic_get(queue.name().as_str(), lapin::options::BasicGetOptions::default()).await.unwrap().unwrap();
        let payload = &delivery.data;
        let event_str = std::str::from_utf8(payload).unwrap();
        assert!(event_str.contains("PackageVersionPublished"));
        assert!(event_str.contains("package-version/com.my-org/my-app/1.2.3"));
    }
    
    #[tokio::test]
    async fn test_upload_existing_artifact_should_create_new_package_version_integration() {
        // Arrange
        let context = setup_test_environment().await;

        let coordinates1 = PackageCoordinates {
            namespace: Some("com.my-org".to_string()),
            name: "my-app".to_string(),
            version: "1.0.0".to_string(),
            qualifiers: Default::default(),
        };
        let metadata1 = json!({
            "coordinates": coordinates1,
            "file_name": "my-app-1.0.0.jar",
            "content_length": 27,
        });
        let file_content = b"This is a test file content.";
        let file_part1 = Part::bytes(file_content.as_ref()).file_name("my-app-1.0.0.jar");
        let metadata_part1 = Part::text(metadata1.to_string());
        let form1 = Form::new()
            .part("file", file_part1)
            .part("metadata", metadata_part1);

        // First upload
        let response1 = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .multipart(form1)
            .send()
            .await
            .unwrap();
        assert_eq!(response1.status(), reqwest::StatusCode::CREATED);

        // Second upload with same content but new version
        let coordinates2 = PackageCoordinates {
            namespace: Some("com.my-org".to_string()),
            name: "my-app".to_string(),
            version: "2.0.0".to_string(), // New version
            qualifiers: Default::default(),
        };
        let metadata2 = json!({
            "coordinates": coordinates2,
            "file_name": "my-app-2.0.0.jar",
            "content_length": 27,
        });
        let file_part2 = Part::bytes(file_content.as_ref()).file_name("my-app-2.0.0.jar");
        let metadata_part2 = Part::text(metadata2.to_string());
        let form2 = Form::new()
            .part("file", file_part2)
            .part("metadata", metadata_part2);

        // Act
        let response2 = context.http_client
            .post(format!("{}/artifacts", context.app_url))
            .multipart(form2)
            .send()
            .await
            .unwrap();

        // Assert
        assert_eq!(response2.status(), reqwest::StatusCode::CREATED);
        let body2 = response2.json::<UploadArtifactResponse>().await.unwrap();
        assert!(body2.hrn.contains("package-version/com.my-org/my-app/2.0.0"));

        // Verify MongoDB: physical_artifacts count should still be 1, package_versions count should be 2
        let db = context.mongo_client.database("test_db");
        let physical_artifacts_collection: mongodb::Collection<mongodb::bson::Document> = db.collection("physical_artifacts");
        let package_versions_collection: mongodb::Collection<mongodb::bson::Document> = db.collection("package_versions");

        let physical_artifact_count = physical_artifacts_collection.count_documents(doc! {}).await.unwrap();
        assert_eq!(physical_artifact_count, 1);

        let package_version_count = package_versions_collection.count_documents(doc! {}).await.unwrap();
        assert_eq!(package_version_count, 2);

        // Verify RabbitMQ events (two events should be published)
        let channel = context.rabbitmq_connection.create_channel().await.unwrap();
        let queue = channel.queue_declare("test_queue", lapin::options::QueueDeclareOptions::default(), lapin::types::FieldTable::default()).await.unwrap();
        channel.queue_bind(queue.name().as_str(), "test_exchange", "artifact.uploaded", lapin::options::QueueBindOptions::default(), lapin::types::FieldTable::default()).await.unwrap();
        
        let delivery1 = channel.basic_get(queue.name().as_str(), lapin::options::BasicGetOptions::default()).await.unwrap().unwrap();
        let payload1 = &delivery1.data;
        let event_str1 = std::str::from_utf8(payload1).unwrap();
        assert!(event_str1.contains("PackageVersionPublished"));
        assert!(event_str1.contains("package-version/com.my-org/my-app/1.0.0"));

        let delivery2 = channel.basic_get(queue.name().as_str(), lapin::options::BasicGetOptions::default()).await.unwrap().unwrap();
        let payload2 = &delivery2.data;
        let event_str2 = std::str::from_utf8(payload2).unwrap();
        assert!(event_str2.contains("PackageVersionPublished"));
        assert!(event_str2.contains("package-version/com.my-org/my-app/2.0.0"));
    }
}
