#![allow(dead_code)]
#![allow(unused_imports)]

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use reqwest;
use serde_json::Value;
use shared::domain::model::{ArtifactId, RepositoryId, UserId};
use testcontainers::clients::Cli;
use testcontainers_modules::{kafka, minio};
use tokio::runtime::Runtime;
use uuid::Uuid;

mod common;

#[tokio::test]
async fn test_basic_search_e2e() {
    let docker = Cli::default();
    let mongo_container = docker.run(minio::MinIO::default());
    let minio_container = docker.run(minio::MinIO::default());
    let kafka_container = docker.run(kafka::Kafka::default());

    let mongo_port = mongo_container.get_host_port_ipv4(27017);
    let s3_port = minio_container.get_host_port_ipv4(9000);
    let kafka_port = kafka_container.get_host_port_ipv4(9093);

    let port = common::start_server(mongo_port, s3_port, kafka_port).await;
    let client = reqwest::Client::new();

    // 1. Create Repository
    let repo_name = "test-repo-search";
    let repo_id = Uuid::new_v4().to_string();
    let user_id = Uuid::new_v4().to_string();

        let resp = client
            .post(format!("http://127.0.0.1:{}/v1/repositories", port))
            .json(&serde_json::json!({
                "id": repo_id,
                "name": repo_name,
                "user_id": user_id
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::CREATED);

        // 2. Upload Artifact
        let artifact_id = Uuid::new_v4().to_string();
        let file_content = "This is a test file for search.";
        let file_name = "search_test.txt";

        let resp = client
            .put(format!(
                "http://127.0.0.1:{}/v1/repositories/{}/artifacts/{}",
                port, repo_id, artifact_id
            ))
            .body(file_content)
            .header("Content-Type", "text/plain")
            .header("X-Filename", file_name)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::CREATED);

        // 3. Wait for indexing
        tokio::time::sleep(Duration::from_secs(5)).await;

        // 4. Search for the artifact
        let resp = client
            .get(format!(
                "http://127.0.0.1:{}/v1/search?q={}",
                port, file_name
            ))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let search_results: Value = resp.json().await.unwrap();

        let results_array = search_results["results"].as_array().unwrap();
        assert_eq!(results_array.len(), 1);
        assert_eq!(results_array[0]["name"].as_str().unwrap(), file_name);
        assert_eq!(
            results_array[0]["repository_id"].as_str().unwrap(),
            repo_id
        );
}
