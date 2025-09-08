use std::sync::Arc;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use std::path::Path;

use crate::domain::{
    events::{ArtifactEvent, PackageVersionPublished},
    package_version::{ArtifactStatus, PackageMetadata, PackageVersion},
    physical_artifact::PhysicalArtifact,
};
use crate::features::upload_artifact::{
    dto::{UploadArtifactCommand, UploadArtifactResponse},
    error::UploadArtifactError,
    ports::{ArtifactStorage, EventPublisher, PortResult, UploadArtifactRepository},
};
use shared::{
    enums::{ArtifactRole, ArtifactType, HashAlgorithm},
    hrn::{Hrn, OrganizationId, RepositoryId, PhysicalArtifactId, UserId},
    lifecycle::Lifecycle,
    models::{ArtifactReference, ContentHash},
};
use futures::stream::Stream;
use shared::models::{PackageCoordinates, ArtifactReference, ContentHash, PackageVersion};
use crate::domain::events::ArtifactEvent;
use crate::domain::events::ArtifactEvent::*;
use crate::domain::error::DomainError;
use uuid::Uuid;

pub struct UploadArtifactUseCase {
    repository: Arc<dyn UploadArtifactRepository>,
    storage: Arc<dyn ArtifactStorage>,
    publisher: Arc<dyn EventPublisher>,
}

impl UploadArtifactUseCase {
    pub fn new(
        repository: Arc<dyn UploadArtifactRepository>,
        storage: Arc<dyn ArtifactStorage>,
        publisher: Arc<dyn EventPublisher>,
    ) -> Self {
        Self {
            repository,
            storage,
            publisher,
        }
    }

    pub async fn execute(
        &self,
        command: UploadArtifactCommand,
        content: Bytes,
    ) -> PortResult<UploadArtifactResponse> {
        tracing::info!("Executing use case");
        // 1. Calculate checksum
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let hash_bytes = hasher.finalize();
        let content_hash_str = hex::encode(hash_bytes);
        tracing::debug!("Content hash: {}", content_hash_str);

        // 2. Check for existing physical artifact
        let physical_artifact_hrn = match self.repository.find_physical_artifact_by_hash(&content_hash_str).await {
            Ok(Some(existing)) => {
                tracing::debug!("Found existing physical artifact");
                existing.hrn
            }
            Ok(None) => {
                tracing::debug!("Creating new physical artifact");
                // 3. Upload to storage if it's a new artifact
                let storage_location = self.storage.upload(content.clone(), &content_hash_str).await.map_err(|e| {
                    tracing::error!("Storage upload error: {:?}", e);
                    e
                })?;
                tracing::debug!("Storage location: {}", storage_location);

                // 4. Create and save the physical artifact record
                let new_physical_artifact_hrn = PhysicalArtifactId::new(&content_hash_str).map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?;
                let new_physical_artifact = PhysicalArtifact {
                    hrn: new_physical_artifact_hrn.0.clone(),
                    organization_hrn: OrganizationId::new("default").map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?,
                    content_hash: ContentHash {
                        algorithm: HashAlgorithm::Sha256,
                        value: content_hash_str.clone(),
                    },
                    size_in_bytes: command.content_length,
                    checksums: std::collections::HashMap::new(),
                    storage_location,
                    lifecycle: Lifecycle::new(UserId::new_system_user().0),
                };
                self.repository.save_physical_artifact(&new_physical_artifact).await.map_err(|e| {
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
        let org_name = command.coordinates.namespace.clone().unwrap_or("default".to_string());
        tracing::debug!("Org name: {}", org_name);
        let org_id = OrganizationId::new(&org_name).map_err(|e| {
            tracing::error!("OrganizationId creation error: {:?}", e);
            UploadArtifactError::RepositoryError(e.to_string())
        })?;
        let repo_id = RepositoryId::new(&org_id, "default").map_err(|e| {
            tracing::error!("RepositoryId creation error: {:?}", e);
            UploadArtifactError::RepositoryError(e.to_string())
        })?;
        let hrn_str = format!("{}/package-version/{}/{}", repo_id.0.as_str(), command.coordinates.name, command.coordinates.version);
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

        self.repository.save_package_version(&package_version).await.map_err(|e| {
            tracing::error!("Repository save package version error: {:?}", e);
            e
        })?;

        // 6. Publish event
        let event = ArtifactEvent::PackageVersionPublished(PackageVersionPublished {
            hrn: package_version.hrn,
            repository_hrn: package_version.repository_hrn,
            coordinates: package_version.coordinates.clone(),
            artifacts: package_version.artifacts.clone(),
            publisher_hrn: UserId::from_hrn(package_version.lifecycle.created_by),
            at: OffsetDateTime::now_utc(),
        });
        self.publisher.publish(&event).await.map_err(|e| {
            tracing::error!("Event publish error: {:?}", e);
            e
        })?;

        // 7. Return response
        Ok(UploadArtifactResponse { hrn: hrn.to_string() })
    }

    pub async fn execute_from_temp_file(
        &self,
        command: UploadArtifactCommand,
        temp_file_path: &Path,
        computed_sha256_hex: &str,
    ) -> PortResult<UploadArtifactResponse> {
        tracing::info!("Executing use case from temp file");
        let content_hash_str = computed_sha256_hex.to_string();
        tracing::debug!("Content hash: {}", content_hash_str);

        // 1. Check for existing physical artifact
        let physical_artifact_hrn = match self.repository.find_physical_artifact_by_hash(&content_hash_str).await {
            Ok(Some(existing)) => {
                tracing::debug!("Found existing physical artifact");
                existing.hrn
            }
            Ok(None) => {
                tracing::debug!("Creating new physical artifact from temp file");
                // 2. Upload to storage if it's a new artifact
                let storage_location = self.storage.upload_from_path(temp_file_path, &content_hash_str).await.map_err(|e| {
                    tracing::error!("Storage upload_from_path error: {:?}", e);
                    e
                })?;
                tracing::debug!("Storage location: {}", storage_location);

                // 3. Create and save the physical artifact record
                let new_physical_artifact_hrn = PhysicalArtifactId::new(&content_hash_str).map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?;
                let new_physical_artifact = PhysicalArtifact {
                    hrn: new_physical_artifact_hrn.0.clone(),
                    organization_hrn: OrganizationId::new("default").map_err(|e| UploadArtifactError::RepositoryError(e.to_string()))?,
                    content_hash: ContentHash {
                        algorithm: HashAlgorithm::Sha256,
                        value: content_hash_str.clone(),
                    },
                    size_in_bytes: command.content_length,
                    checksums: std::collections::HashMap::new(),
                    storage_location,
                    lifecycle: Lifecycle::new(UserId::new_system_user().0),
                };
                self.repository.save_physical_artifact(&new_physical_artifact).await.map_err(|e| {
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
        let org_name = command.coordinates.namespace.clone().unwrap_or("default".to_string());
        tracing::debug!("Org name: {}", org_name);
        let org_id = OrganizationId::new(&org_name).map_err(|e| {
            tracing::error!("OrganizationId creation error: {:?}", e);
            UploadArtifactError::RepositoryError(e.to_string())
        })?;
        let repo_id = RepositoryId::new(&org_id, "default").map_err(|e| {
            tracing::error!("RepositoryId creation error: {:?}", e);
            UploadArtifactError::RepositoryError(e.to_string())
        })?;
        let hrn_str = format!("{}/package-version/{}/{}", repo_id.0.as_str(), command.coordinates.name, command.coordinates.version);
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

        self.repository.save_package_version(&package_version).await.map_err(|e| {
            tracing::error!("Repository save package version error: {:?}", e);
            e
        })?;

        // 5. Publish event
        let event = ArtifactEvent::PackageVersionPublished(PackageVersionPublished {
            hrn: package_version.hrn,
            repository_hrn: package_version.repository_hrn,
            coordinates: package_version.coordinates.clone(),
            artifacts: package_version.artifacts.clone(),
            publisher_hrn: UserId::from_hrn(package_version.lifecycle.created_by),
            at: OffsetDateTime::now_utc(),
        });
        self.publisher.publish(&event).await.map_err(|e| {
            tracing::error!("Event publish error: {:?}", e);
            e
        })?;

        Ok(UploadArtifactResponse { hrn: hrn.to_string() })
    }
}

// Implementing resumable upload functionality
impl UploadArtifactUseCase {
    pub async fn execute_from_stream(&self, package_coords: PackageCoordinates, stream: impl Stream<Item = Result<Bytes, anyhow::Error>> + Send + Sync + 'static) -> Result<ArtifactUploaded, DomainError> {
        // Generate upload ID
        let upload_id = Uuid::new_v4().to_string();
        
        // Track progress
        let mut total_bytes = 0;
        let mut chunk_number = 0;
        let mut progress_reported = 0;
        
        // Process stream of chunks
        let mut stream = stream.chunks(10); // Process in chunks of 10 items
        
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| DomainError::StorageError(e.to_string()))?;
            
            // Increment chunk counter
            chunk_number += 1;
            
            // Save chunk
            self.storage.save_chunk(&upload_id, chunk_number, chunk).await?;
            
            // Update total bytes
            total_bytes += chunk.len() as u64;
            
            // Calculate progress percentage
            let progress = (total_bytes * 100) / package_coords.size;
            
            // Report progress every 5% or when complete
            if progress - progress_reported >= 5 || progress == 100 {
                progress_reported = progress;
                
                // Publish progress event
                self.publisher.publish(&ArtifactEvent::UploadProgressUpdated {
                    upload_id: upload_id.clone(),
                    progress,
                    bytes_uploaded: total_bytes,
                    total_bytes: package_coords.size,
                }).await?;
            }
        }
        
        // Assemble chunks into final artifact
        let final_path = self.storage.assemble_to_path(&upload_id, chunk_number).await?;
        
        // Create artifact reference
        let artifact_ref = ArtifactReference {
            artifact_hrn: format!("hrn:artifact:{}", upload_id),
            artifact_type: "generic".to_string(),
            role: "primary".to_string(),
            package_coordinates: package_coords.clone(),
            path: final_path.to_str().unwrap().to_string(),
        };
        
        // Create uploaded event
        let event = ArtifactEvent::ArtifactUploaded {
            artifact: artifact_ref.clone(),
        };
        
        // Publish event
        self.publisher.publish(&event).await?;
        
        Ok(ArtifactUploaded {
            artifact: artifact_ref,
            upload_id,
        })
    }
}
        let mut total_bytes = 0;
        let mut chunk_number = 0;
        let mut progress_reported = 0;
        
        // Process stream of chunks
        let mut stream = stream.chunks(10); // Process in chunks of 10 items
        
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| DomainError::StorageError(e.to_string()))?;
            
            // Increment chunk counter
            chunk_number += 1;
            
            // Save chunk
            self.storage.save_chunk(&upload_id, chunk_number, chunk).await?;
            
            // Update total bytes
            total_bytes += chunk.len() as u64;
            
            // Calculate progress percentage
            let progress = (total_bytes * 100) / package_coords.size;
            
            // Report progress every 5% or when complete
            if progress - progress_reported >= 5 || progress == 100 {
                progress_reported = progress;
                
                // Publish progress event
                self.event_publisher.publish(&ArtifactEvent::UploadProgressUpdated {
                    upload_id: upload_id.clone(),
                    progress,
                    bytes_uploaded: total_bytes,
                    total_bytes: package_coords.size,
                }).await?;
            }
        }
        
        // Assemble chunks into final artifact
        let final_path = self.storage.assemble_to_path(&upload_id, chunk_number).await?;
        
        // Create artifact reference
        let artifact_ref = ArtifactReference {
            artifact_hrn: format!("hrn:artifact:{}", upload_id),
            artifact_type: "generic".to_string(),
            role: "primary".to_string(),
            package_coordinates: package_coords.clone(),
            path: final_path.to_str().unwrap().to_string(),
        };
        
        // Create uploaded event
        let event = ArtifactEvent::ArtifactUploaded {
            artifact: artifact_ref.clone(),
        };
        
        // Publish event
        self.event_publisher.publish(&event).await?;
        
        Ok(ArtifactUploaded {
            artifact: artifact_ref,
            upload_id,
        })
    }
}

// Defining ArtifactUploaded struct
#[derive(Debug, Clone)]
pub struct ArtifactUploaded {
    pub artifact: ArtifactReference,
    pub upload_id: String,
}
