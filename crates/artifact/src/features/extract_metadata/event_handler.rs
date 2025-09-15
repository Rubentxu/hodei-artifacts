use super::{dto::ExtractMetadataCommand, error::MetadataError, use_case::ExtractMetadataUseCase};
use crate::domain::events::PackageVersionPublished;
use std::sync::Arc;
use tracing::{error, info};

/// Event handler for PackageVersionPublished events
/// Triggers metadata extraction when a new package version is published
pub struct PackageVersionPublishedEventHandler {
    use_case: Arc<ExtractMetadataUseCase>,
}

impl PackageVersionPublishedEventHandler {
    pub fn new(use_case: Arc<ExtractMetadataUseCase>) -> Self {
        Self { use_case }
    }

    /// Handle PackageVersionPublished event and trigger metadata extraction
    pub async fn handle(&self, event: PackageVersionPublished) -> Result<(), MetadataError> {
        info!("Handling PackageVersionPublished event for: {}", event.hrn);

        // Store the HRN for error reporting
        let hrn = event.hrn.clone();

        // Determine artifact type from package coordinates
        let artifact_type = self.determine_artifact_type(&event);

        // Get storage path for the artifact
        let storage_path = self.get_storage_path(&event);

        // Create command for metadata extraction
        let command = ExtractMetadataCommand {
            package_version_hrn: event.hrn,
            artifact_storage_path: storage_path,
            artifact_type,
        };

        // Execute metadata extraction
        match self.use_case.execute(command).await {
            Ok(result) => {
                info!(
                    "Successfully extracted metadata for package: {}",
                    result.package_version_hrn
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    "Failed to extract metadata for package: {} - Error: {}",
                    hrn, e
                );
                // We don't propagate the error to avoid breaking the event flow
                Ok(())
            }
        }
    }

    /// Determine artifact type based on package coordinates
    fn determine_artifact_type(&self, event: &PackageVersionPublished) -> String {
        // In a real implementation, we would analyze the package coordinates
        // to determine the artifact type

        // For now, we'll use a simple heuristic
        if let Some(namespace) = &event.coordinates.namespace {
            if namespace.contains("maven") {
                return "maven".to_string();
            }
        }

        // Check if coordinates name suggests NPM
        if event.coordinates.name.contains("npm") || event.coordinates.name.contains("node") {
            return "npm".to_string();
        }

        // Default to maven for now
        "maven".to_string()
    }

    /// Get storage path for the artifact
    fn get_storage_path(&self, _event: &PackageVersionPublished) -> String {
        // In a real implementation, we would construct the storage path
        // based on the event information
        "s3://bucket/path/to/artifact".to_string()
    }
}
