#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use bytes::Bytes;
    use artifact::features::upload_artifact::{
        di::UploadArtifactDIContainer,
        dto::UploadArtifactCommand,
    };
    use shared::models::PackageCoordinates;

    #[tokio::test]
    async fn test_upload_artifact_basic_functionality() {
        // Arrange - Create a test DI container with mock dependencies
        let di_container = UploadArtifactDIContainer::for_testing();
        
        let coordinates = PackageCoordinates {
            namespace: Some("com.my-org".to_string()),
            name: "my-app".to_string(),
            version: "1.2.3".to_string(),
            qualifiers: Default::default(),
        };
        
        let command = UploadArtifactCommand {
            coordinates,
            file_name: "my-app-1.2.3.jar".to_string(),
            content_length: 27, // Length of "This is a test file content."
        };
        
        let content = Bytes::from("This is a test file content.");

        // Act
        let result = di_container.endpoint.upload_artifact(command, content).await;

        // Assert
        assert!(result.is_ok(), "Upload should succeed: {:?}", result.err());
        
        let response = result.unwrap();
        assert!(response.hrn.contains("package-version/com.my-org/my-app/1.2.3"));
    }
}