use shared_test::setup_test_environment;
use distribution::features::npm::package_meta::publish_handler::handle_npm_publish;
use distribution::features::npm::tarball::handler::handle_npm_tarball_download;
use shared::RepositoryId;

use serde_json::json;
use base64::engine::general_purpose;
use base64::engine::Engine as _;

#[tokio::test]
async fn it_npm_full_flow() {
    let env = setup_test_environment(None).await;

    // Test npm Upload
    let package_name = "my-npm-package".to_string();
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

    handle_npm_publish(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        repository_id.clone(), // Pass the RepositoryId
        package_name.clone(),
        request_bytes, // Pass the JSON bytes here
    ).await.unwrap();
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

    handle_npm_publish(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.artifact_event_publisher.clone(),
        env.authorization.clone(),
        package_name.clone(),
        request_bytes, // Pass the JSON bytes here
    ).await.unwrap();

    // Test npm Download
    let download_result = handle_npm_tarball_download(
        env.artifact_storage.clone(),
        env.artifact_repository.clone(),
        env.authorization.clone(),
        package_name.clone(),
        format!("{}-{}.tgz", package_name, version),
    ).await;

    if let Err(e) = &download_result {
        eprintln!("npm Download failed with error: {:?}", e);
    }
    assert!(download_result.is_ok());
    assert_eq!(download_result.unwrap(), tarball_bytes_raw);
}
