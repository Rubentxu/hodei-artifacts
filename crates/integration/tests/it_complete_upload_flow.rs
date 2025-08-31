use shared_test::setup_test_environment;
use tokio::process::Command;
use tokio::time::{sleep, Duration};
use reqwest::Client;
use std::env;

#[tokio::test]
async fn it_complete_upload_index_search_flow() {
    // 1. Setup Test Environment (Docker Compose services)
    let test_env = setup_test_environment(None).await;

    // Extract dynamic ports for the API to connect to
    let mongo_port = test_env.dynamic_ports.as_ref().unwrap().mongo_port;
    let kafka_port = test_env.dynamic_ports.as_ref().unwrap().kafka_port;
    let s3_port = test_env.dynamic_ports.as_ref().unwrap().s3_port;

    // 2. Start hodei-artifacts-api binary in the background
    let api_binary_path = env::current_dir()
        .unwrap()
        .join("target/debug/hodei-artifacts-api");

    let mut api_process = Command::new(&api_binary_path)
        .env("MONGO_URI", format!("mongodb://localhost:{}", mongo_port))
        .env("KAFKA_BROKER", format!("localhost:{}", kafka_port))
        .env("S3_ENDPOINT", format!("http://localhost:{}", s3_port))
        .env("RUST_LOG", "info,hodei_artifacts=debug") // Set log level for visibility
        .spawn()
        .expect("Failed to start hodei-artifacts-api");

    // 3. Wait for the API to be ready
    let client = Client::new();
    let api_base_url = "http://localhost:8080"; // Default API port
    let health_check_url = format!("{}/health", api_base_url);

    for _ in 0..30 { // Try for up to 30 seconds
        match client.get(&health_check_url).send().await {
            Ok(response) if response.status().is_success() => {
                println!("API is ready!");
                break;
            }
            _ => {
                println!("Waiting for API to be ready...");
                sleep(Duration::from_secs(1)).await;
            }
        }
    }

    // Ensure the API started successfully
    let health_response = client.get(&health_check_url).send().await.expect("API health check failed");
    assert!(health_response.status().is_success(), "API did not start successfully");

    // 4. Implement upload, wait for indexing, and search logic
    let artifact_name = format!("test-artifact-{}", chrono::Utc::now().timestamp_millis());
    let artifact_content = b"This is a test artifact content.";
    let checksum = sha256::digest(artifact_content);

    // Create a dummy repository first
    let repo_name = format!("test-repo-{}", chrono::Utc::now().timestamp_millis());
    let create_repo_url = format!("{}/v1/repositories", api_base_url);
    let create_repo_body = serde_json::json!({
        "name": repo_name,
        "description": "A test repository"
    });
    let create_repo_response = client.post(&create_repo_url)
        .json(&create_repo_body)
        .send()
        .await
        .expect("Failed to create repository");
    assert!(create_repo_response.status().is_success(), "Failed to create repository: {:?}", create_repo_response.text().await);

    // Upload the artifact
    let upload_url = format!("{}/v1/artifacts", api_base_url);
    let file_part = reqwest::multipart::Part::bytes(artifact_content.to_vec())
        .file_name(artifact_name.clone())
        .mime_str("application/octet-stream")
        .unwrap();

    let metadata = serde_json::json!({
        "repository_name": repo_name,
        "artifact_name": artifact_name,
        "checksum": checksum,
        "size": artifact_content.len(),
        "mime_type": "application/octet-stream"
    });

    let metadata_part = reqwest::multipart::Part::text(metadata.to_string())
        .file_name("metadata.json")
        .mime_str("application/json")
        .unwrap();

    let form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .part("metadata", metadata_part);

    let upload_response = client.post(&upload_url)
        .multipart(form)
        .send()
        .await
        .expect("Failed to upload artifact");

    assert!(upload_response.status().is_success(), "Failed to upload artifact: {:?}", upload_response.text().await);
    println!("Artifact uploaded successfully.");

    // 5. Wait for indexing (Kafka consumer processing)
    sleep(Duration::from_secs(5)).await; // Give Kafka consumer time to process

    // 6. Perform search
    let search_url = format!("{}/v1/search?q={}", api_base_url, artifact_name);
    let search_response = client.get(&search_url)
        .send()
        .await
        .expect("Failed to perform search");

    assert!(search_response.status().is_success(), "Search failed: {:?}", search_response.text().await);

    let search_results: serde_json::Value = search_response.json().await.expect("Failed to parse search results");
    println!("Search results: {:?}", search_results);

    // Assert that the uploaded artifact is found in the search results
    let found = search_results["results"].as_array().unwrap().iter()
        .any(|item| item["artifact_name"] == artifact_name);
    assert!(found, "Uploaded artifact not found in search results.");
    println!("Uploaded artifact found in search results.");

    // 7. Teardown: Kill the API process
    api_process.kill().await.expect("Failed to kill API process");
}
