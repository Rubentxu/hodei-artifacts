use bytes::Bytes;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use time::OffsetDateTime;
use tokio::fs;

use crate::domain::events::{
    ArtifactEvent, ArtifactValidationFailed, DuplicateArtifactDetected, PackageVersionPublished,
};
use crate::domain::{
    package_version::{ArtifactStatus, PackageMetadata, PackageVersion},
    physical_artifact::PhysicalArtifact,
};
use crate::features::content_type_detection::ContentTypeDetectionUseCase;
use crate::features::upload_artifact::{
    dto::{UploadArtifactCommand, UploadArtifactResponse},
    error::UploadArtifactError,
    ports::{
        ArtifactRepository, ArtifactStorage, ArtifactValidator, EventPublisher, PortResult,
        VersionValidator,
    },
};
use shared::{
    enums::{ArtifactRole, ArtifactType, HashAlgorithm},
    hrn::{Hrn, OrganizationId, PhysicalArtifactId, RepositoryId, UserId},
    lifecycle::Lifecycle,
    models::{ArtifactReference, ContentHash},
};

pub struct UploadArtifactUseCase {
    repository: Arc<dyn ArtifactRepository>,
    storage: Arc<dyn ArtifactStorage>,
    event_publisher: Arc<dyn EventPublisher>,
    validator: Arc<dyn ArtifactValidator>,
    version_validator: Arc<dyn VersionValidator>,
    content_type_service: Arc<ContentTypeDetectionUseCase>,
}

impl UploadArtifactUseCase {
    pub fn new(
        repository: Arc<dyn ArtifactRepository>,
        storage: Arc<dyn ArtifactStorage>,
        event_publisher: Arc<dyn EventPublisher>,
        validator: Arc<dyn ArtifactValidator>,
        version_validator: Arc<dyn VersionValidator>,
        content_type_service: Arc<ContentTypeDetectionUseCase>,
    ) -> Self {
        Self {
            repository,
            storage,
            event_publisher,
            validator,
            version_validator,
            content_type_service,
        }
    }

    pub async fn execute(
        &self,
        command: UploadArtifactCommand,
        content: Bytes,
    ) -> PortResult<UploadArtifactResponse> {
        tracing::info!("Executing use case");

        // 0. Pre-commit validation
        if let Err(errors) = self.validator.validate(&command, &content).await {
            tracing::warn!(?errors, "Artifact validation failed");
            // Publish event (non-blocking on publish failure)
            let event = ArtifactEvent::ArtifactValidationFailed(ArtifactValidationFailed {
                coordinates: command.coordinates.clone(),
                errors: errors.clone(),
                at: OffsetDateTime::now_utc(),
            });
            if let Err(e) = self.event_publisher.publish(&event).await {
                tracing::error!(error = %e, "Failed to publish ArtifactValidationFailed event");
            }
            return Err(UploadArtifactError::ValidationFailed(errors.join(", ")));
        }

        // 0.1. Validación de versión (nueva)
        if let Err(e) = self
            .version_validator
            .validate_version(&command.coordinates.version)
            .await
        {
            let error_msg = e.clone();
            tracing::warn!(error = %e, "Version validation failed");
            // Publish event (non-blocking on publish failure)
            let event = ArtifactEvent::ArtifactValidationFailed(ArtifactValidationFailed {
                coordinates: command.coordinates.clone(),
                errors: vec![e],
                at: OffsetDateTime::now_utc(),
            });
            if let Err(publish_error) = self.event_publisher.publish(&event).await {
                tracing::error!(error = %publish_error, "Failed to publish ArtifactValidationFailed event");
            }
            return Err(UploadArtifactError::VersioningError(error_msg));
        }

        // 1. Calculate checksum
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let hash_bytes = hasher.finalize();
        let content_hash_str = hex::encode(hash_bytes);
        tracing::debug!("Content hash: {}", content_hash_str);

        // 1.1. Detect content type using magic numbers and extension
        let content_type_result = self
            .content_type_service
            .detect_content_type(
                content.clone(),
                Some(&command.file_name),
                None, // No client-provided content type in this flow
            )
            .await
            .map_err(|e| {
                tracing::warn!("Content type detection failed: {}", e);
                UploadArtifactError::ValidationFailed(format!(
                    "Content type detection failed: {}",
                    e
                ))
            })?;

        let detected_mime_type = content_type_result.detected_mime_type;
        tracing::debug!("Detected MIME type: {}", detected_mime_type);

        // 2. Check for existing physical artifact
        let physical_artifact_hrn = match self
            .repository
            .find_physical_artifact_by_hash(&content_hash_str)
            .await
        {
            Ok(Some(existing)) => {
                tracing::debug!("Found existing physical artifact");

                // Publish DuplicateArtifactDetected event
                tracing::debug!("Publishing DuplicateArtifactDetected event");
                let duplicate_event =
                    ArtifactEvent::DuplicateArtifactDetected(DuplicateArtifactDetected {
                        content_hash: content_hash_str.clone(),
                        existing_physical_artifact_hrn: existing.hrn.to_string(),
                        new_package_coordinates: command.coordinates.clone(),
                        size_in_bytes: command.content_length,
                        at: OffsetDateTime::now_utc(),
                    });

                if let Err(e) = self.event_publisher.publish(&duplicate_event).await {
                    tracing::warn!(error = %e, "Failed to publish DuplicateArtifactDetected event");
                }

                existing.hrn
            }
            Ok(None) => {
                tracing::debug!("Creating new physical artifact");
                // 3. Upload to storage if it's a new artifact
                let storage_location = self
                    .storage
                    .upload(content.clone(), &content_hash_str)
                    .await
                    .map_err(|e| {
                        tracing::error!("Storage upload error: {:?}", e);
                        e
                    })?;
                tracing::debug!("Storage location: {}", storage_location);

                // 4. Create and save the physical artifact record
                let new_physical_artifact_hrn = PhysicalArtifactId::new(&content_hash_str)
                    .map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?;
                let new_physical_artifact = PhysicalArtifact {
                    hrn: new_physical_artifact_hrn.0.clone(),
                    organization_hrn: OrganizationId::new("default")
                        .map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?,
                    content_hash: ContentHash {
                        algorithm: HashAlgorithm::Sha256,
                        value: content_hash_str.clone(),
                    },
                    size_in_bytes: command.content_length,
                    checksums: std::collections::HashMap::new(),
                    storage_location,
                    mime_type: detected_mime_type.clone(),
                    lifecycle: Lifecycle::new(UserId::new_system_user().0),
                };
                self.repository
                    .save_physical_artifact(&new_physical_artifact)
                    .await
                    .map_err(|e| {
                        tracing::error!("Repository save physical artifact error: {:?}", e);
                        e
                    })?;
                tracing::debug!("Saved new physical artifact");
                new_physical_artifact_hrn.0
            }
            Err(e) => {
                tracing::error!("Repository find physical artifact error: {:?}", e);
                return Err(e);
            }
        };

        // 5. Create and save the package version
        let org_name = command
            .coordinates
            .namespace
            .clone()
            .unwrap_or("default".to_string());
        tracing::debug!("Org name: {}", org_name);
        let org_id = OrganizationId::new(&org_name).map_err(|e| {
            tracing::error!("OrganizationId creation error: {:?}", e);
            UploadArtifactError::RepositoryError(e.to_string())
        })?;
        let repo_id = RepositoryId::new(org_id.as_str(), "default").map_err(|e| {
            tracing::error!("RepositoryId creation error: {:?}", e);
            UploadArtifactError::RepositoryError(e.to_string())
        })?;
        let hrn_str = format!(
            "{}/package-version/{}/{}",
            repo_id.0.as_str(),
            command.coordinates.name,
            command.coordinates.version
        );
        tracing::debug!("Package version hrn string: {}", hrn_str);
        let hrn = Hrn::new(&hrn_str).map_err(|e| {
            tracing::error!("Hrn creation error: {:?}", e);
            UploadArtifactError::RepositoryError(e.to_string())
        })?;
        tracing::debug!("Package version hrn: {}", hrn);

        let package_version = PackageVersion {
            hrn: hrn.clone(),
            organization_hrn: org_id,
            repository_hrn: repo_id,
            coordinates: command.coordinates.clone(),
            status: ArtifactStatus::Active,
            metadata: PackageMetadata {
                description: None,
                licenses: vec![],
                authors: vec![],
                project_url: None,
                repository_url: None,
                last_downloaded_at: None,
                download_count: 0,
                custom_properties: std::collections::HashMap::new(),
            },
            artifacts: vec![ArtifactReference {
                artifact_hrn: PhysicalArtifactId(physical_artifact_hrn.clone()),
                artifact_type: ArtifactType::Primary,
                role: Some(ArtifactRole::Main),
            }],
            dependencies: vec![],
            tags: vec![],
            lifecycle: Lifecycle::new(UserId::new_system_user().0),
            oci_manifest_hrn: None,
        };

        self.repository
            .save_package_version(&package_version)
            .await
            .map_err(|e| {
                tracing::error!("Repository save package version error: {:?}", e);
                e
            })?;

        // 6. Publish event
        let event = ArtifactEvent::PackageVersionPublished(PackageVersionPublished {
            hrn: package_version.hrn,
            repository_hrn: package_version.repository_hrn,
            coordinates: package_version.coordinates.clone(),
            artifacts: package_version.artifacts.clone(),
            publisher_hrn: UserId(package_version.lifecycle.created_by.clone()),
            at: OffsetDateTime::now_utc(),
        });
        self.event_publisher.publish(&event).await.map_err(|e| {
            tracing::error!("Event publish error: {:?}", e);
            e
        })?;

        // 7. Return response
        Ok(UploadArtifactResponse {
            hrn: hrn.to_string(),
            url: None,
        })
    }

    pub async fn execute_from_temp_file(
        &self,
        command: UploadArtifactCommand,
        temp_file_path: &Path,
        precomputed_checksum: Option<String>,
    ) -> PortResult<UploadArtifactResponse> {
        tracing::info!("Executing use case from temp file");

        // 0. Pre-commit validation reading from temp file
        let file_content = fs::read(temp_file_path).await.map_err(|e| {
            UploadArtifactError::StorageError(format!("Failed to read temp file: {}", e))
        })?;
        if let Err(errors) = self
            .validator
            .validate(&command, &Bytes::from(file_content.clone()))
            .await
        {
            tracing::warn!(?errors, "Artifact validation failed (temp file)");
            let event = ArtifactEvent::ArtifactValidationFailed(ArtifactValidationFailed {
                coordinates: command.coordinates.clone(),
                errors: errors.clone(),
                at: OffsetDateTime::now_utc(),
            });
            if let Err(e) = self.event_publisher.publish(&event).await {
                tracing::error!(error = %e, "Failed to publish ArtifactValidationFailed event");
            }
            return Err(UploadArtifactError::ValidationFailed(errors.join(", ")));
        }

        // 0.1. Validación de versión (nueva)
        if let Err(e) = self
            .version_validator
            .validate_version(&command.coordinates.version)
            .await
        {
            let error_msg = e.clone();
            tracing::warn!(error = %e, "Version validation failed (temp file)");
            // Publish event (non-blocking on publish failure)
            let event = ArtifactEvent::ArtifactValidationFailed(ArtifactValidationFailed {
                coordinates: command.coordinates.clone(),
                errors: vec![e],
                at: OffsetDateTime::now_utc(),
            });
            if let Err(publish_error) = self.event_publisher.publish(&event).await {
                tracing::error!(error = %publish_error, "Failed to publish ArtifactValidationFailed event");
            }
            return Err(UploadArtifactError::ValidationFailed(format!(
                "Version validation failed: {}",
                error_msg
            )));
        }

        let content_hash_str = match precomputed_checksum {
            Some(checksum) => {
                tracing::debug!("Using precomputed checksum: {}", checksum);
                checksum
            }
            None => {
                tracing::debug!("Calculating checksum from temp file");
                let mut hasher = Sha256::new();
                hasher.update(&file_content);
                hex::encode(hasher.finalize())
            }
        };
        tracing::debug!("Content hash: {}", content_hash_str);

        // 1.1. Detect content type using magic numbers and extension
        let content_type_result = self
            .content_type_service
            .detect_content_type(
                Bytes::from(file_content.clone()),
                Some(&command.file_name),
                None, // No client-provided content type in this flow
            )
            .await
            .map_err(|e| {
                tracing::warn!("Content type detection failed: {}", e);
                UploadArtifactError::ValidationFailed(format!(
                    "Content type detection failed: {}",
                    e
                ))
            })?;

        let detected_mime_type = content_type_result.detected_mime_type;
        tracing::debug!("Detected MIME type: {}", detected_mime_type);

        // 1. Check for existing physical artifact
        let physical_artifact_hrn = match self
            .repository
            .find_physical_artifact_by_hash(&content_hash_str)
            .await
        {
            Ok(Some(existing)) => {
                tracing::debug!("Found existing physical artifact");

                // Publish DuplicateArtifactDetected event
                tracing::debug!("Publishing DuplicateArtifactDetected event");
                let duplicate_event =
                    ArtifactEvent::DuplicateArtifactDetected(DuplicateArtifactDetected {
                        content_hash: content_hash_str.clone(),
                        existing_physical_artifact_hrn: existing.hrn.to_string(),
                        new_package_coordinates: command.coordinates.clone(),
                        size_in_bytes: command.content_length,
                        at: OffsetDateTime::now_utc(),
                    });

                if let Err(e) = self.event_publisher.publish(&duplicate_event).await {
                    tracing::warn!(error = %e, "Failed to publish DuplicateArtifactDetected event");
                }

                existing.hrn
            }
            Ok(None) => {
                tracing::debug!("Creating new physical artifact from temp file");
                // 2. Upload to storage if it's a new artifact
                let storage_location = self
                    .storage
                    .upload_from_path(temp_file_path, &content_hash_str)
                    .await
                    .map_err(|e| {
                        tracing::error!("Storage upload_from_path error: {:?}", e);
                        e
                    })?;
                tracing::debug!("Storage location: {}", storage_location);

                // 3. Create and save the physical artifact record
                let new_physical_artifact_hrn = PhysicalArtifactId::new(&content_hash_str)
                    .map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?;
                let new_physical_artifact = PhysicalArtifact {
                    hrn: new_physical_artifact_hrn.0.clone(),
                    organization_hrn: OrganizationId::new("default")
                        .map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?,
                    content_hash: ContentHash {
                        algorithm: HashAlgorithm::Sha256,
                        value: content_hash_str.clone(),
                    },
                    size_in_bytes: command.content_length,
                    checksums: std::collections::HashMap::new(),
                    storage_location,
                    mime_type: detected_mime_type.clone(),
                    lifecycle: Lifecycle::new(UserId::new_system_user().0),
                };
                self.repository
                    .save_physical_artifact(&new_physical_artifact)
                    .await
                    .map_err(|e| {
                        tracing::error!("Repository save physical artifact error: {:?}", e);
                        e
                    })?;
                tracing::debug!("Saved new physical artifact");
                new_physical_artifact_hrn.0
            }
            Err(e) => {
                tracing::error!("Repository find physical artifact error: {:?}", e);
                return Err(e);
            }
        };

        // 4. Create and save the package version (same as in execute)
        let org_name = command
            .coordinates
            .namespace
            .clone()
            .unwrap_or("default".to_string());
        tracing::debug!("Org name: {}", org_name);
        let org_id = OrganizationId::new(&org_name).map_err(|e| {
            tracing::error!("OrganizationId creation error: {:?}", e);
            UploadArtifactError::RepositoryError(e.to_string())
        })?;
        let repo_id = RepositoryId::new(org_id.as_str(), "default").map_err(|e| {
            tracing::error!("RepositoryId creation error: {:?}", e);
            UploadArtifactError::RepositoryError(e.to_string())
        })?;
        let hrn_str = format!(
            "{}/package-version/{}/{}",
            repo_id.0.as_str(),
            command.coordinates.name,
            command.coordinates.version
        );
        tracing::debug!("Package version hrn string: {}", hrn_str);
        let hrn = Hrn::new(&hrn_str).map_err(|e| {
            tracing::error!("Hrn creation error: {:?}", e);
            UploadArtifactError::RepositoryError(e.to_string())
        })?;
        tracing::debug!("Package version hrn: {}", hrn);

        let package_version = PackageVersion {
            hrn: hrn.clone(),
            organization_hrn: org_id,
            repository_hrn: repo_id,
            coordinates: command.coordinates.clone(),
            status: ArtifactStatus::Active,
            metadata: PackageMetadata {
                description: None,
                licenses: vec![],
                authors: vec![],
                project_url: None,
                repository_url: None,
                last_downloaded_at: None,
                download_count: 0,
                custom_properties: std::collections::HashMap::new(),
            },
            artifacts: vec![ArtifactReference {
                artifact_hrn: PhysicalArtifactId(physical_artifact_hrn.clone()),
                artifact_type: ArtifactType::Primary,
                role: Some(ArtifactRole::Main),
            }],
            dependencies: vec![],
            tags: vec![],
            lifecycle: Lifecycle::new(UserId::new_system_user().0),
            oci_manifest_hrn: None,
        };

        self.repository
            .save_package_version(&package_version)
            .await
            .map_err(|e| {
                tracing::error!("Repository save package version error: {:?}", e);
                e
            })?;

        // 5. Publish event
        let event = ArtifactEvent::PackageVersionPublished(PackageVersionPublished {
            hrn: package_version.hrn,
            repository_hrn: package_version.repository_hrn,
            coordinates: package_version.coordinates.clone(),
            artifacts: package_version.artifacts.clone(),
            publisher_hrn: UserId(package_version.lifecycle.created_by.clone()),
            at: OffsetDateTime::now_utc(),
        });
        self.event_publisher.publish(&event).await.map_err(|e| {
            tracing::error!("Event publish error: {:?}", e);
            UploadArtifactError::EventPublishError(e.to_string())
        })?;

        Ok(UploadArtifactResponse {
            hrn: hrn.to_string(),
            url: None,
        })
    }
}
