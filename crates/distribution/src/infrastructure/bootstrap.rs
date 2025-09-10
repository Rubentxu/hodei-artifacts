//! Bootstrap module for distribution infrastructure

use std::sync::Arc;
use crate::features::{
    handle_maven_request::MavenRequestDIContainer,
    handle_npm_request::NpmRequestDIContainer,
    handle_docker_request::DockerRequestDIContainer,
    generate_maven_metadata::GenerateMavenMetadataDIContainer,
    generate_npm_metadata::GenerateNpmMetadataDIContainer,
    generate_docker_manifest::GenerateDockerManifestDIContainer,
};
use super::config::DistributionConfig;

/// Distribution infrastructure bootstrap
pub struct DistributionBootstrap {
    config: DistributionConfig,
}

impl DistributionBootstrap {
    /// Create a new bootstrap instance
    pub fn new(config: DistributionConfig) -> Self {
        Self { config }
    }

    /// Initialize Maven request DI container for production
    pub async fn init_maven_request_container(&self) -> anyhow::Result<MavenRequestDIContainer> {
        tracing::info!("Initializing Maven request DI container");
        
        let container = MavenRequestDIContainer::for_production(
            self.config.s3.clone(),
            self.config.mongodb.clone(),
            self.config.cedar.clone(),
        ).await?;
        
        tracing::info!("Maven request DI container initialized successfully");
        Ok(container)
    }

    /// Initialize NPM request DI container for production
    pub async fn init_npm_request_container(&self) -> anyhow::Result<NpmRequestDIContainer> {
        tracing::info!("Initializing NPM request DI container");
        
        let container = NpmRequestDIContainer::for_production(
            self.config.s3.clone(),
            self.config.mongodb.clone(),
            self.config.cedar.clone(),
        ).await?;
        
        tracing::info!("NPM request DI container initialized successfully");
        Ok(container)
    }

    /// Initialize Docker request DI container for production
    pub async fn init_docker_request_container(&self) -> anyhow::Result<DockerRequestDIContainer> {
        tracing::info!("Initializing Docker request DI container");
        
        let container = DockerRequestDIContainer::for_production(
            self.config.s3.clone(),
            self.config.mongodb.clone(),
            self.config.cedar.clone(),
        ).await?;
        
        tracing::info!("Docker request DI container initialized successfully");
        Ok(container)
    }

    /// Initialize Maven metadata generation DI container for production
    pub async fn init_maven_metadata_container(&self) -> anyhow::Result<GenerateMavenMetadataDIContainer> {
        tracing::info!("Initializing Maven metadata generation DI container");
        
        let container = GenerateMavenMetadataDIContainer::for_production(
            self.config.s3.clone(),
            self.config.mongodb.clone(),
            self.config.redis.clone(),
        ).await?;
        
        tracing::info!("Maven metadata generation DI container initialized successfully");
        Ok(container)
    }

    /// Initialize NPM metadata generation DI container for production
    pub async fn init_npm_metadata_container(&self) -> anyhow::Result<GenerateNpmMetadataDIContainer> {
        tracing::info!("Initializing NPM metadata generation DI container");
        
        let container = GenerateNpmMetadataDIContainer::for_production(
            self.config.s3.clone(),
            self.config.mongodb.clone(),
            self.config.redis.clone(),
        ).await?;
        
        tracing::info!("NPM metadata generation DI container initialized successfully");
        Ok(container)
    }

    /// Initialize Docker manifest generation DI container for production
    pub async fn init_docker_manifest_container(&self) -> anyhow::Result<GenerateDockerManifestDIContainer> {
        tracing::info!("Initializing Docker manifest generation DI container");
        
        let container = GenerateDockerManifestDIContainer::for_production(
            self.config.s3.clone(),
            self.config.mongodb.clone(),
            self.config.redis.clone(),
        ).await?;
        
        tracing::info!("Docker manifest generation DI container initialized successfully");
        Ok(container)
    }

    /// Initialize all DI containers for production
    pub async fn init_all_containers(&self) -> anyhow::Result<DistributionContainers> {
        tracing::info!("Initializing all distribution DI containers");
        
        let maven_request = self.init_maven_request_container().await?;
        let npm_request = self.init_npm_request_container().await?;
        let docker_request = self.init_docker_request_container().await?;
        let maven_metadata = self.init_maven_metadata_container().await?;
        let npm_metadata = self.init_npm_metadata_container().await?;
        let docker_manifest = self.init_docker_manifest_container().await?;
        
        let containers = DistributionContainers {
            maven_request,
            npm_request,
            docker_request,
            maven_metadata,
            npm_metadata,
            docker_manifest,
        };
        
        tracing::info!("All distribution DI containers initialized successfully");
        Ok(containers)
    }

    /// Create bootstrap for testing
    pub fn for_testing() -> Self {
        Self {
            config: DistributionConfig::for_testing(),
        }
    }

    /// Initialize all DI containers for testing
    pub async fn init_all_containers_for_testing(&self) -> anyhow::Result<DistributionContainers> {
        tracing::info!("Initializing all distribution DI containers for testing");
        
        let maven_request = MavenRequestDIContainer::for_testing();
        let npm_request = NpmRequestDIContainer::for_testing();
        let docker_request = DockerRequestDIContainer::for_testing();
        let maven_metadata = GenerateMavenMetadataDIContainer::for_testing();
        let npm_metadata = GenerateNpmMetadataDIContainer::for_testing();
        let docker_manifest = GenerateDockerManifestDIContainer::for_testing();
        
        let containers = DistributionContainers {
            maven_request,
            npm_request,
            docker_request,
            maven_metadata,
            npm_metadata,
            docker_manifest,
        };
        
        tracing::info!("All distribution DI containers for testing initialized successfully");
        Ok(containers)
    }
}

/// Collection of all distribution DI containers
pub struct DistributionContainers {
    pub maven_request: MavenRequestDIContainer,
    pub npm_request: NpmRequestDIContainer,
    pub docker_request: DockerRequestDIContainer,
    pub maven_metadata: GenerateMavenMetadataDIContainer,
    pub npm_metadata: GenerateNpmMetadataDIContainer,
    pub docker_manifest: GenerateDockerManifestDIContainer,
}