//! Unified domain model for repository bounded context
//! Merges existing implementations with DDD patterns

use crate::domain::{
    repository::Repository as LegacyRepository,
    package::Package as LegacyPackage,
    version::Version as LegacyVersion,
};
use shared::{
    hrn::Hrn,
    lifecycle::Lifecycle,
    enums::{Ecosystem, RepositoryType},
};
use time::OffsetDateTime;
use std::collections::HashSet;

/// Unified Repository aggregate root
#[derive(Debug, Clone)]
pub struct Repository {
    pub id: RepositoryId,
    pub name: RepositoryName,
    pub display_name: String,
    pub description: Option<String>,
    pub repository_type: RepositoryType,
    pub ecosystem: Ecosystem,
    pub organization_id: OrganizationId,
    pub visibility: RepositoryVisibility,
    pub state: RepositoryState,
    pub settings: RepositorySettings,
    pub packages: HashSet<PackageId>,
    pub lifecycle: Lifecycle,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl Repository {
    pub fn from_legacy(legacy: LegacyRepository) -> Self {
        Self {
            id: RepositoryId(legacy.hrn),
            name: RepositoryName(legacy.name),
            display_name: legacy.display_name,
            description: legacy.description,
            repository_type: legacy.repository_type,
            ecosystem: legacy.ecosystem,
            organization_id: OrganizationId(legacy.organization_hrn),
            visibility: RepositoryVisibility::from_legacy(legacy.visibility),
            state: RepositoryState::Active,
            settings: RepositorySettings::default(),
            packages: HashSet::new(),
            lifecycle: legacy.lifecycle,
            created_at: legacy.created_at,
            updated_at: legacy.updated_at,
        }
    }

    pub fn to_legacy(&self) -> LegacyRepository {
        LegacyRepository {
            hrn: self.id.0.clone(),
            name: self.name.0.clone(),
            display_name: self.display_name.clone(),
            description: self.description.clone(),
            repository_type: self.repository_type,
            ecosystem: self.ecosystem,
            organization_hrn: self.organization_id.0.clone(),
            visibility: self.visibility.to_legacy(),
            lifecycle: self.lifecycle.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub fn add_package(&mut self, package_id: PackageId) {
        self.packages.insert(package_id);
    }

    pub fn remove_package(&mut self, package_id: &PackageId) {
        self.packages.remove(package_id);
    }

    pub fn can_user_access(&self, user_id: &UserId) -> bool {
        match self.visibility {
            RepositoryVisibility::Public => true,
            RepositoryVisibility::Private => {
                // Check if user has access through organization membership
                // This would be implemented with actual permission checking
                true
            }
            RepositoryVisibility::Internal => {
                // Check if user is part of the organization
                true
            }
        }
    }
}

/// Unified Package entity
#[derive(Debug, Clone)]
pub struct Package {
    pub id: PackageId,
    pub name: PackageName,
    pub display_name: String,
    pub description: Option<String>,
    pub repository_id: RepositoryId,
    pub ecosystem: Ecosystem,
    pub tags: HashSet<String>,
    pub versions: HashSet<VersionId>,
    pub lifecycle: Lifecycle,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl Package {
    pub fn from_legacy(legacy: LegacyPackage) -> Self {
        Self {
            id: PackageId(legacy.hrn),
            name: PackageName(legacy.name),
            display_name: legacy.display_name,
            description: legacy.description,
            repository_id: RepositoryId(legacy.repository_hrn),
            ecosystem: legacy.ecosystem,
            tags: legacy.tags.into_iter().collect(),
            versions: HashSet::new(),
            lifecycle: legacy.lifecycle,
            created_at: legacy.created_at,
            updated_at: legacy.updated_at,
        }
    }

    pub fn to_legacy(&self) -> LegacyPackage {
        LegacyPackage {
            hrn: self.id.0.clone(),
            name: self.name.0.clone(),
            display_name: self.display_name.clone(),
            description: self.description.clone(),
            repository_hrn: self.repository_id.0.clone(),
            ecosystem: self.ecosystem,
            tags: self.tags.iter().cloned().collect(),
            lifecycle: self.lifecycle.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub fn add_version(&mut self, version_id: VersionId) {
        self.versions.insert(version_id);
    }

    pub fn remove_version(&mut self, version_id: &VersionId) {
        self.versions.remove(version_id);
    }

    pub fn add_tag(&mut self, tag: String) {
        self.tags.insert(tag);
    }

    pub fn remove_tag(&mut self, tag: &str) {
        self.tags.remove(tag);
    }
}

/// Unified Version entity
#[derive(Debug, Clone)]
pub struct Version {
    pub id: VersionId,
    pub package_id: PackageId,
    pub version_string: VersionString,
    pub description: Option<String>,
    pub release_notes: Option<String>,
    pub is_prerelease: bool,
    pub is_deprecated: bool,
    pub deprecation_message: Option<String>,
    pub artifact_ids: HashSet<ArtifactId>,
    pub lifecycle: Lifecycle,
    pub created_at: OffsetDateTime,
    pub published_at: Option<OffsetDateTime>,
    pub deprecated_at: Option<OffsetDateTime>,
}

impl Version {
    pub fn from_legacy(legacy: LegacyVersion) -> Self {
        Self {
            id: VersionId(legacy.hrn),
            package_id: PackageId(legacy.package_hrn),
            version_string: VersionString(legacy.version),
            description: legacy.description,
            release_notes: legacy.release_notes,
            is_prerelease: legacy.is_prerelease,
            is_deprecated: legacy.is_deprecated,
            deprecation_message: legacy.deprecation_message,
            artifact_ids: HashSet::new(),
            lifecycle: legacy.lifecycle,
            created_at: legacy.created_at,
            published_at: legacy.published_at,
            deprecated_at: legacy.deprecated_at,
        }
    }

    pub fn to_legacy(&self) -> LegacyVersion {
        LegacyVersion {
            hrn: self.id.0.clone(),
            package_hrn: self.package_id.0.clone(),
            version: self.version_string.0.clone(),
            description: self.description.clone(),
            release_notes: self.release_notes.clone(),
            is_prerelease: self.is_prerelease,
            is_deprecated: self.is_deprecated,
            deprecation_message: self.deprecation_message.clone(),
            lifecycle: self.lifecycle.clone(),
            created_at: self.created_at,
            published_at: self.published_at,
            deprecated_at: self.deprecated_at,
        }
    }

    pub fn add_artifact(&mut self, artifact_id: ArtifactId) {
        self.artifact_ids.insert(artifact_id);
    }

    pub fn remove_artifact(&mut self, artifact_id: &ArtifactId) {
        self.artifact_ids.remove(artifact_id);
    }

    pub fn publish(&mut self) {
        self.published_at = Some(OffsetDateTime::now_utc());
    }

    pub fn deprecate(&mut self, message: String) {
        self.is_deprecated = true;
        self.deprecation_message = Some(message);
        self.deprecated_at = Some(OffsetDateTime::now_utc());
    }
}

/// Repository settings value object
#[derive(Debug, Clone, Default)]
pub struct RepositorySettings {
    pub allow_public_read: bool,
    pub require_approval: bool,
    pub max_package_size: Option<u64>,
    pub allowed_file_types: HashSet<String>,
    pub custom_properties: std::collections::HashMap<String, String>,
}

/// Repository visibility enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryVisibility {
    Public,
    Private,
    Internal,
}

impl RepositoryVisibility {
    pub fn from_legacy(legacy: crate::domain::repository::RepositoryVisibility) -> Self {
        match legacy {
            crate::domain::repository::RepositoryVisibility::Public => Self::Public,
            crate::domain::repository::RepositoryVisibility::Private => Self::Private,
            crate::domain::repository::RepositoryVisibility::Internal => Self::Internal,
        }
    }

    pub fn to_legacy(&self) -> crate::domain::repository::RepositoryVisibility {
        match self {
            Self::Public => crate::domain::repository::RepositoryVisibility::Public,
            Self::Private => crate::domain::repository::RepositoryVisibility::Private,
            Self::Internal => crate::domain::repository::RepositoryVisibility::Internal,
        }
    }
}

/// Repository state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryState {
    Active,
    Archived,
    Suspended,
}

/// Version string value object
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VersionString(pub String);

/// Value objects
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepositoryId(pub Hrn);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageId(pub Hrn);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VersionId(pub Hrn);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageName(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrganizationId(pub Hrn);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserId(pub Hrn);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactId(pub Hrn);

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::Hrn;

    #[test]
    fn test_repository_creation() {
        let repo_hrn = Hrn::new("hrn:hodei:repository:us-east-1:acme:repository/main").unwrap();
        let org_hrn = Hrn::new("hrn:hodei:iam:global:acme:organization/acme").unwrap();
        
        let repository = Repository {
            id: RepositoryId(repo_hrn),
            name: RepositoryName("main".to_string()),
            display_name: "Main Repository".to_string(),
            description: Some("Main repository for artifacts".to_string()),
            repository_type: RepositoryType::Artifact,
            ecosystem: Ecosystem::Maven,
            organization_id: OrganizationId(org_hrn),
            visibility: RepositoryVisibility::Public,
            state: RepositoryState::Active,
            settings: RepositorySettings::default(),
            packages: HashSet::new(),
            lifecycle: Lifecycle::new(Hrn::new("hrn:hodei:iam:global:acme:user/system").unwrap()),
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        };

        assert_eq!(repository.name.0, "main");
        assert_eq!(repository.visibility, RepositoryVisibility::Public);
    }

    #[test]
    fn test_package_management() {
        let package_hrn = Hrn::new("hrn:hodei:repository:us-east-1:acme:package/my-lib").unwrap();
        let repo_hrn = Hrn::new("hrn:hodei:repository:us-east-1:acme:repository/main").unwrap();
        
        let mut package = Package {
            id: PackageId(package_hrn),
            name: PackageName("my-lib".to_string()),
            display_name: "My Library".to_string(),
            description: Some("A useful library".to_string()),
            repository_id: RepositoryId(repo_hrn),
            ecosystem: Ecosystem::Maven,
            tags: HashSet::new(),
            versions: HashSet::new(),
            lifecycle: Lifecycle::new(Hrn::new("hrn:hodei:iam:global:acme:user/system").unwrap()),
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        };

        package.add_tag("utility".to_string());
        assert!(package.tags.contains("utility"));
    }
}
