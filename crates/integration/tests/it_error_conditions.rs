use shared_test::setup_test_environment;
use distribution::features::maven::upload::handler::handle_maven_upload;
use shared::RepositoryId;

#[tokio::test]
async fn it_upload_duplicate_maven_artifact_fails() {
    let env = setup_test_environment(None).await;

    // Test Maven Upload - First upload (should succeed)
    let group_id = "com.example".to_string();
    let artifact_id = "my-maven-lib-duplicate".to_string();
    let version = "1.0.0".to_string();
    let file_name = "my-maven-lib-duplicate-1.0.0.jar".to_string();
    let bytes = vec![1, 2, 3, 4, 5, 6, 7, 8];

    let repository_id = RepositoryId::new(); // Create once

    let upload_result = handle_maven_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id.clone(), // Pass the same ID
        group_id.clone(),
        artifact_id.clone(),
        version.clone(),
        file_name.clone(),
        bytes.clone(),
    ).await;
    assert!(upload_result.is_ok());

    // Test Maven Upload - Second upload (should fail with Duplicate error)
    let duplicate_upload_result = handle_maven_upload(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id.clone(), // Pass the same ID
        group_id.clone(),
        artifact_id.clone(),
        version.clone(),
        file_name.clone(),
        bytes.clone(),
    ).await;

    assert!(duplicate_upload_result.is_err());
    if let Err(e) = duplicate_upload_result {
        assert!(matches!(e, distribution::error::DistributionError::Artifact(artifact_error) if matches!(artifact_error, artifact::error::ArtifactError::Duplicate)));
    } else {
        panic!("Expected an error, but got success");
    }
}

use distribution::features::npm::package_meta::publish_handler::handle_npm_publish;
use serde_json::json;
use base64::engine::general_purpose;
use base64::engine::Engine as _;

#[tokio::test]
async fn it_upload_duplicate_npm_package_fails() {
    let env = setup_test_environment(None).await;

    let package_name = "my-npm-package-duplicate".to_string();
    let version = "1.0.0".to_string();
    let tarball_bytes_raw = vec![1, 2, 3, 4, 5, 6, 7, 8]; // Actual tarball content
    let tarball_bytes_base64 = general_purpose::STANDARD.encode(&tarball_bytes_raw); // Base64 encode it

    let npm_publish_request_json = json!({
        "_id": package_name,
        "name": package_name,
        "description": "A test npm package",
        "dist-tags": {
            "latest": version,
        },
        "versions": {
            version.clone(): {
                "name": package_name,
                "version": version,
                "dist": {
                    "shasum": "dummy_shasum", // This will be recalculated by the handler
                    "tarball": format!("http://localhost:8080/{}/-/{}-{}.tgz", package_name, package_name, version), // Placeholder URL
                },
            },
        },
        "_attachments": {
            format!("{}-{}.tgz", package_name, version): {
                "content_type": "application/octet-stream",
                "data": tarball_bytes_base64,
                "length": tarball_bytes_raw.len(),
            },
        },
    });

    let request_bytes = serde_json::to_vec(&npm_publish_request_json).unwrap();

    let repository_id = RepositoryId::new(); // Create once

    // First upload
    let upload_result = handle_npm_publish(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id.clone(), // Pass the same ID
        package_name.clone(),
        request_bytes.clone(),
    ).await;
    assert!(upload_result.is_ok());

    // Second upload
    let duplicate_upload_result = handle_npm_publish(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id.clone(), // Pass the same ID
        package_name.clone(),
        request_bytes.clone(),
    ).await;

    assert!(duplicate_upload_result.is_err());
    if let Err(e) = duplicate_upload_result {
        assert!(matches!(e, distribution::error::DistributionError::Artifact(artifact_error) if matches!(artifact_error, artifact::error::ArtifactError::Duplicate)));
    } else {
        panic!("Expected an error, but got success");
    }
}