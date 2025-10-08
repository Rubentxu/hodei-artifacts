//! Artifact entity - represents a stored artifact/object
//!
//! NOTE: This entity is temporarily placed in hodei-iam for initial implementation.
//! In the future, it should be moved to its own hodei-artifacts bounded context.

use kernel::Hrn;
use kernel::domain::entity::{HodeiEntity, HodeiEntityType, Resource};
use kernel::domain::value_objects::{ResourceTypeName, ServiceName};
use kernel::{AttributeName, AttributeType, AttributeValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Artifact entity representing a stored artifact/object
///
/// Artifacts are files or objects stored in the system that can have
/// policies applied to them for access control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Artifact {
    /// Hierarchical Resource Name (unique identifier)
    pub hrn: Hrn,

    /// Artifact name/filename
    pub name: String,

    /// Content type / MIME type (e.g., "application/pdf", "image/png")
    pub content_type: String,

    /// Size in bytes
    pub size_bytes: u64,

    /// Owner user HRN
    pub owner_hrn: Hrn,

    /// Optional parent container/folder HRN
    pub parent_hrn: Option<Hrn>,

    /// Visibility level
    pub visibility: ArtifactVisibility,

    /// Tags for categorization
    pub tags: Vec<String>,

    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Artifact visibility levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactVisibility {
    /// Private - only owner and explicitly granted users can access
    Private,
    /// Internal - anyone in the organization can access
    Internal,
    /// Public - anyone can access
    Public,
}

impl Artifact {
    /// Create a new artifact
    pub(crate) fn new(
        hrn: Hrn,
        name: String,
        content_type: String,
        size_bytes: u64,
        owner_hrn: Hrn,
    ) -> Self {
        Self {
            hrn,
            name,
            content_type,
            size_bytes,
            owner_hrn,
            parent_hrn: None,
            visibility: ArtifactVisibility::Private,
            tags: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Set parent container/folder
    pub(crate) fn with_parent(mut self, parent_hrn: Hrn) -> Self {
        self.parent_hrn = Some(parent_hrn);
        self
    }

    /// Set visibility
    pub(crate) fn with_visibility(mut self, visibility: ArtifactVisibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// Add a tag
    pub(crate) fn add_tag(&mut self, tag: String) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }
    }

    /// Add metadata entry
    pub(crate) fn add_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }

    /// Get owner HRN
    pub(crate) fn owner(&self) -> &Hrn {
        &self.owner_hrn
    }

    /// Check if artifact is public
    pub(crate) fn is_public(&self) -> bool {
        self.visibility == ArtifactVisibility::Public
    }

    /// Check if artifact is private
    pub(crate) fn is_private(&self) -> bool {
        self.visibility == ArtifactVisibility::Private
    }
}

// ============================================================================
// Kernel Traits Implementation
// ============================================================================

impl HodeiEntityType for Artifact {
    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
        // TODO: Change to "artifacts" when moved to hodei-artifacts crate
    }

    fn resource_type_name() -> ResourceTypeName {
        ResourceTypeName::new("Artifact").expect("Valid resource type")
    }

    fn is_principal_type() -> bool {
        false // Artifacts are not principals
    }

    fn is_resource_type() -> bool {
        true
    }

    fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
        vec![
            (
                AttributeName::new("name").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("content_type").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("size_bytes").expect("Valid attribute name"),
                AttributeType::long(),
            ),
            (
                AttributeName::new("owner").expect("Valid attribute name"),
                AttributeType::EntityRef("User"),
            ),
            (
                AttributeName::new("visibility").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("is_public").expect("Valid attribute name"),
                AttributeType::bool(),
            ),
            (
                AttributeName::new("is_private").expect("Valid attribute name"),
                AttributeType::bool(),
            ),
            (
                AttributeName::new("tags").expect("Valid attribute name"),
                AttributeType::set(AttributeType::string()),
            ),
        ]
    }
}

impl HodeiEntity for Artifact {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
        let mut attrs = HashMap::new();

        attrs.insert(
            AttributeName::new("name").expect("Valid attribute name"),
            AttributeValue::string(&self.name),
        );

        attrs.insert(
            AttributeName::new("content_type").expect("Valid attribute name"),
            AttributeValue::string(&self.content_type),
        );

        attrs.insert(
            AttributeName::new("size_bytes").expect("Valid attribute name"),
            AttributeValue::long(self.size_bytes as i64),
        );

        attrs.insert(
            AttributeName::new("owner").expect("Valid attribute name"),
            AttributeValue::entity_ref(self.owner_hrn.to_string()),
        );

        let visibility_str = match self.visibility {
            ArtifactVisibility::Private => "private",
            ArtifactVisibility::Internal => "internal",
            ArtifactVisibility::Public => "public",
        };
        attrs.insert(
            AttributeName::new("visibility").expect("Valid attribute name"),
            AttributeValue::string(visibility_str),
        );

        attrs.insert(
            AttributeName::new("is_public").expect("Valid attribute name"),
            AttributeValue::bool(self.is_public()),
        );

        attrs.insert(
            AttributeName::new("is_private").expect("Valid attribute name"),
            AttributeValue::bool(self.is_private()),
        );

        let tag_values: Vec<AttributeValue> = self
            .tags
            .iter()
            .map(|t| AttributeValue::string(t))
            .collect();
        attrs.insert(
            AttributeName::new("tags").expect("Valid attribute name"),
            AttributeValue::set(tag_values),
        );

        attrs
    }

    fn parent_hrns(&self) -> Vec<Hrn> {
        if let Some(parent) = &self.parent_hrn {
            vec![parent.clone()]
        } else {
            Vec::new()
        }
    }
}

// Artifact is a Resource (policies can be about artifacts)
impl Resource for Artifact {}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::AttributeName;

    #[test]
    fn test_artifact_creation() {
        let artifact_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Artifact".to_string(),
            "document.pdf".to_string(),
        );

        let owner_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let artifact = Artifact::new(
            artifact_hrn.clone(),
            "document.pdf".to_string(),
            "application/pdf".to_string(),
            1024000,
            owner_hrn.clone(),
        );

        assert_eq!(artifact.hrn, artifact_hrn);
        assert_eq!(artifact.name, "document.pdf");
        assert_eq!(artifact.content_type, "application/pdf");
        assert_eq!(artifact.size_bytes, 1024000);
        assert_eq!(artifact.owner_hrn, owner_hrn);
        assert_eq!(artifact.visibility, ArtifactVisibility::Private);
        assert!(artifact.tags.is_empty());
        assert!(artifact.parent_hrn.is_none());
    }

    #[test]
    fn test_artifact_with_parent() {
        let artifact_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Artifact".to_string(),
            "file.txt".to_string(),
        );

        let owner_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let folder_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Folder".to_string(),
            "documents".to_string(),
        );

        let artifact = Artifact::new(
            artifact_hrn.clone(),
            "file.txt".to_string(),
            "text/plain".to_string(),
            1024,
            owner_hrn,
        )
        .with_parent(folder_hrn.clone());

        assert_eq!(artifact.parent_hrn, Some(folder_hrn.clone()));
        assert_eq!(artifact.parent_hrns(), vec![folder_hrn]);
    }

    #[test]
    fn test_artifact_visibility() {
        let artifact_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Artifact".to_string(),
            "file.txt".to_string(),
        );

        let owner_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let mut artifact = Artifact::new(
            artifact_hrn,
            "file.txt".to_string(),
            "text/plain".to_string(),
            1024,
            owner_hrn,
        );

        assert!(artifact.is_private());
        assert!(!artifact.is_public());

        artifact = artifact.with_visibility(ArtifactVisibility::Public);
        assert!(artifact.is_public());
        assert!(!artifact.is_private());
    }

    #[test]
    fn test_artifact_tags() {
        let artifact_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Artifact".to_string(),
            "file.txt".to_string(),
        );

        let owner_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let mut artifact = Artifact::new(
            artifact_hrn,
            "file.txt".to_string(),
            "text/plain".to_string(),
            1024,
            owner_hrn,
        );

        artifact.add_tag("important".to_string());
        artifact.add_tag("draft".to_string());
        artifact.add_tag("important".to_string()); // Duplicate

        assert_eq!(artifact.tags.len(), 2);
        assert!(artifact.tags.contains(&"important".to_string()));
        assert!(artifact.tags.contains(&"draft".to_string()));
    }

    #[test]
    fn test_artifact_metadata() {
        let artifact_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Artifact".to_string(),
            "file.txt".to_string(),
        );

        let owner_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let mut artifact = Artifact::new(
            artifact_hrn,
            "file.txt".to_string(),
            "text/plain".to_string(),
            1024,
            owner_hrn,
        );

        artifact.add_metadata("department".to_string(), "engineering".to_string());
        artifact.add_metadata("project".to_string(), "hodei".to_string());

        assert_eq!(artifact.metadata.len(), 2);
        assert_eq!(
            artifact.metadata.get("department"),
            Some(&"engineering".to_string())
        );
        assert_eq!(artifact.metadata.get("project"), Some(&"hodei".to_string()));
    }

    #[test]
    fn test_artifact_implements_hodei_entity() {
        let artifact_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "Artifact".to_string(),
            "test.pdf".to_string(),
        );

        let owner_hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "account123".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let artifact = Artifact::new(
            artifact_hrn.clone(),
            "test.pdf".to_string(),
            "application/pdf".to_string(),
            2048,
            owner_hrn.clone(),
        );

        assert_eq!(artifact.hrn(), &artifact_hrn);
        assert_eq!(artifact.parent_hrns().len(), 0);

        let attrs = artifact.attributes();
        assert!(attrs.len() >= 5);
        assert_eq!(
            attrs.get(&AttributeName::new("name").expect("valid")),
            Some(&AttributeValue::string("test.pdf"))
        );
        assert_eq!(
            attrs.get(&AttributeName::new("content_type").expect("valid")),
            Some(&AttributeValue::string("application/pdf"))
        );
        assert_eq!(
            attrs.get(&AttributeName::new("is_private").expect("valid")),
            Some(&AttributeValue::bool(true))
        );
    }

    #[test]
    fn test_artifact_entity_type_metadata() {
        assert_eq!(Artifact::service_name().as_str(), "iam"); // TODO: change to "artifacts"
        assert_eq!(Artifact::resource_type_name().as_str(), "Artifact");
        assert!(!Artifact::is_principal_type());
        assert!(Artifact::is_resource_type());

        let schema = Artifact::attributes_schema();
        assert!(schema.len() >= 7);
    }
}
