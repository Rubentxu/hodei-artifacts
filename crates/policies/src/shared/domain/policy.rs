//! # Policy Domain Entity
//!
//! This module defines the core `Policy` domain entity and its associated value objects.
//! The `Policy` entity represents an authorization policy with its metadata and lifecycle.
//!
//! ## Design Principles
//!
//! - **Rich Domain Model**: The entity encapsulates business rules and invariants
//! - **Immutability**: Once created, policies are immutable (updates create new versions)
//! - **Value Objects**: Uses `PolicyId` and `PolicyMetadata` as value objects
//! - **Self-Contained**: All validation logic is within the entity itself

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A unique identifier for a policy.
///
/// This is a value object that wraps a string ID. It provides type safety,
/// preventing accidental mixing of policy IDs with other string identifiers.
///
/// # Example
///
/// ```rust,ignore
/// use policies::domain::policy::PolicyId;
///
/// let id = PolicyId::new("policy-123");
/// assert_eq!(id.to_string(), "policy-123");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PolicyId(String);

impl PolicyId {
    /// Creates a new `PolicyId` from a string.
    ///
    /// # Arguments
    ///
    /// * `id` - The string representation of the policy ID
    ///
    /// # Returns
    ///
    /// A new `PolicyId` instance.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the inner string representation of the ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `PolicyId` and returns the inner string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for PolicyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for PolicyId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for PolicyId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl AsRef<str> for PolicyId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Metadata associated with a policy.
///
/// This value object contains descriptive and organizational information about
/// a policy, such as its description and tags for categorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyMetadata {
    /// Optional human-readable description of the policy's purpose
    description: Option<String>,

    /// Tags for categorization, filtering, and organization
    tags: Vec<String>,

    /// Timestamp when the policy was created
    created_at: DateTime<Utc>,

    /// Timestamp of the last update (None if never updated)
    updated_at: Option<DateTime<Utc>>,
}

impl PolicyMetadata {
    /// Creates new metadata with the given description and tags.
    ///
    /// # Arguments
    ///
    /// * `description` - Optional description of the policy
    /// * `tags` - List of tags for categorization
    ///
    /// # Returns
    ///
    /// A new `PolicyMetadata` instance with `created_at` set to the current time.
    pub fn new(description: Option<String>, tags: Vec<String>) -> Self {
        Self {
            description,
            tags,
            created_at: Utc::now(),
            updated_at: None,
        }
    }

    /// Creates metadata with only a description.
    pub fn with_description(description: impl Into<String>) -> Self {
        Self::new(Some(description.into()), Vec::new())
    }

    /// Creates metadata with only tags.
    pub fn with_tags(tags: Vec<String>) -> Self {
        Self::new(None, tags)
    }

    /// Creates empty metadata (no description or tags).
    pub fn empty() -> Self {
        Self::new(None, Vec::new())
    }

    /// Returns the description, if any.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns a reference to the tags.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Returns the creation timestamp.
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Returns the last update timestamp, if any.
    pub fn updated_at(&self) -> Option<DateTime<Utc>> {
        self.updated_at
    }

    /// Updates the description and marks the metadata as updated.
    pub fn set_description(&mut self, description: Option<String>) {
        self.description = description;
        self.updated_at = Some(Utc::now());
    }

    /// Updates the tags and marks the metadata as updated.
    pub fn set_tags(&mut self, tags: Vec<String>) {
        self.tags = tags;
        self.updated_at = Some(Utc::now());
    }

    /// Adds a tag if it doesn't already exist.
    pub fn add_tag(&mut self, tag: impl Into<String>) {
        let tag = tag.into();
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
            self.updated_at = Some(Utc::now());
        }
    }

    /// Removes a tag if it exists.
    pub fn remove_tag(&mut self, tag: &str) -> bool {
        if let Some(pos) = self.tags.iter().position(|t| t == tag) {
            self.tags.remove(pos);
            self.updated_at = Some(Utc::now());
            true
        } else {
            false
        }
    }
}

impl Default for PolicyMetadata {
    fn default() -> Self {
        Self::empty()
    }
}

/// The core Policy domain entity.
///
/// A `Policy` represents an authorization rule with its associated metadata.
/// Policies are immutable once created - updates create new versions rather
/// than modifying the existing policy.
///
/// # Invariants
///
/// - The policy ID must not be empty
/// - The policy content must not be empty or whitespace-only
/// - Metadata is always present (though may be empty)
///
/// # Example
///
/// ```rust,ignore
/// use policies::domain::policy::{Policy, PolicyId, PolicyMetadata};
///
/// let id = PolicyId::new("policy-123");
/// let content = "permit(principal, action, resource);".to_string();
/// let metadata = PolicyMetadata::with_description("Allow all access");
///
/// let policy = Policy::new(id, content, metadata);
/// assert_eq!(policy.id().to_string(), "policy-123");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    /// Unique identifier for this policy
    id: PolicyId,

    /// The Cedar policy document (full policy text)
    content: String,

    /// Associated metadata (description, tags, timestamps)
    metadata: PolicyMetadata,
}

impl Policy {
    /// Creates a new `Policy` with the given ID, content, and metadata.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique policy identifier
    /// * `content` - The Cedar policy document as a string
    /// * `metadata` - Associated metadata
    ///
    /// # Returns
    ///
    /// A new `Policy` instance.
    ///
    /// # Panics
    ///
    /// This method does not panic, but it's the caller's responsibility to ensure
    /// the content is not empty. For validation, use the `validate` method or
    /// construct through a use case that enforces invariants.
    pub fn new(id: PolicyId, content: String, metadata: PolicyMetadata) -> Self {
        Self {
            id,
            content,
            metadata,
        }
    }

    /// Creates a new `Policy` with the given ID and content, and empty metadata.
    ///
    /// This is a convenience constructor for when metadata is not needed.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique policy identifier
    /// * `content` - The Cedar policy document as a string
    ///
    /// # Returns
    ///
    /// A new `Policy` instance with default metadata.
    pub fn new_without_metadata(id: PolicyId, content: String) -> Self {
        Self::new(id, content, PolicyMetadata::empty())
    }

    /// Returns the policy's unique identifier.
    pub fn id(&self) -> &PolicyId {
        &self.id
    }

    /// Returns the policy's content (Cedar document).
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Returns a reference to the policy's metadata.
    pub fn metadata(&self) -> &PolicyMetadata {
        &self.metadata
    }

    /// Returns a mutable reference to the policy's metadata.
    ///
    /// Note: In a strict immutable domain model, this would not be exposed.
    /// Consider this for internal use only, or create a new policy with
    /// updated metadata instead.
    pub fn metadata_mut(&mut self) -> &mut PolicyMetadata {
        &mut self.metadata
    }

    /// Validates the policy's invariants.
    ///
    /// Checks that:
    /// - The content is not empty or whitespace-only
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, `Err(String)` with an error message if invalid.
    pub fn validate(&self) -> Result<(), String> {
        if self.content.trim().is_empty() {
            return Err("Policy content cannot be empty".to_string());
        }
        Ok(())
    }

    /// Creates a copy of this policy with updated metadata.
    ///
    /// This is the preferred way to "update" a policy in an immutable domain model.
    ///
    /// # Arguments
    ///
    /// * `metadata` - The new metadata to associate with the policy
    ///
    /// # Returns
    ///
    /// A new `Policy` instance with the same ID and content but different metadata.
    pub fn with_metadata(&self, metadata: PolicyMetadata) -> Self {
        Self {
            id: self.id.clone(),
            content: self.content.clone(),
            metadata,
        }
    }

    /// Creates a copy of this policy with a new content.
    ///
    /// In a versioned system, this would typically create a new version
    /// rather than replacing the existing policy.
    ///
    /// # Arguments
    ///
    /// * `content` - The new Cedar policy document
    ///
    /// # Returns
    ///
    /// A new `Policy` instance with the same ID and metadata but different content.
    pub fn with_content(&self, content: String) -> Self {
        Self {
            id: self.id.clone(),
            content,
            metadata: self.metadata.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_id_can_be_created_and_displayed() {
        let id = PolicyId::new("test-123");
        assert_eq!(id.to_string(), "test-123");
        assert_eq!(id.as_str(), "test-123");
    }

    #[test]
    fn policy_id_can_be_converted_from_string() {
        let id: PolicyId = "test-456".into();
        assert_eq!(id.to_string(), "test-456");
    }

    #[test]
    fn policy_metadata_can_be_created_empty() {
        let metadata = PolicyMetadata::empty();
        assert!(metadata.description().is_none());
        assert!(metadata.tags().is_empty());
        assert!(metadata.updated_at().is_none());
    }

    #[test]
    fn policy_metadata_with_description() {
        let metadata = PolicyMetadata::with_description("Test policy");
        assert_eq!(metadata.description(), Some("Test policy"));
    }

    #[test]
    fn policy_metadata_with_tags() {
        let tags = vec!["production".to_string(), "critical".to_string()];
        let metadata = PolicyMetadata::with_tags(tags.clone());
        assert_eq!(metadata.tags(), tags.as_slice());
    }

    #[test]
    fn policy_metadata_can_add_tag() {
        let mut metadata = PolicyMetadata::empty();
        metadata.add_tag("new-tag");
        assert_eq!(metadata.tags(), &["new-tag"]);
    }

    #[test]
    fn policy_metadata_does_not_add_duplicate_tag() {
        let mut metadata = PolicyMetadata::with_tags(vec!["tag1".to_string()]);
        metadata.add_tag("tag1");
        assert_eq!(metadata.tags().len(), 1);
    }

    #[test]
    fn policy_metadata_can_remove_tag() {
        let mut metadata = PolicyMetadata::with_tags(vec!["tag1".to_string(), "tag2".to_string()]);
        let removed = metadata.remove_tag("tag1");
        assert!(removed);
        assert_eq!(metadata.tags(), &["tag2"]);
    }

    #[test]
    fn policy_can_be_created() {
        let id = PolicyId::new("policy-1");
        let content = "permit(principal, action, resource);".to_string();
        let metadata = PolicyMetadata::empty();

        let policy = Policy::new(id.clone(), content.clone(), metadata);

        assert_eq!(policy.id(), &id);
        assert_eq!(policy.content(), content);
    }

    #[test]
    fn policy_can_be_created_without_metadata() {
        let id = PolicyId::new("policy-2");
        let content = "permit(principal, action, resource);".to_string();

        let policy = Policy::new_without_metadata(id.clone(), content.clone());

        assert_eq!(policy.id(), &id);
        assert_eq!(policy.content(), content);
        assert!(policy.metadata().description().is_none());
    }

    #[test]
    fn policy_validation_rejects_empty_content() {
        let policy = Policy::new_without_metadata(PolicyId::new("test"), "".to_string());
        assert!(policy.validate().is_err());
    }

    #[test]
    fn policy_validation_rejects_whitespace_only_content() {
        let policy = Policy::new_without_metadata(PolicyId::new("test"), "   \n\t  ".to_string());
        assert!(policy.validate().is_err());
    }

    #[test]
    fn policy_validation_accepts_valid_content() {
        let policy = Policy::new_without_metadata(
            PolicyId::new("test"),
            "permit(principal, action, resource);".to_string(),
        );
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn policy_with_metadata_creates_new_instance() {
        let original = Policy::new_without_metadata(
            PolicyId::new("test"),
            "permit(principal, action, resource);".to_string(),
        );

        let new_metadata = PolicyMetadata::with_description("Updated description");
        let updated = original.with_metadata(new_metadata.clone());

        assert_eq!(updated.id(), original.id());
        assert_eq!(updated.content(), original.content());
        assert_eq!(
            updated.metadata().description(),
            Some("Updated description")
        );
    }

    #[test]
    fn policy_with_content_creates_new_instance() {
        let original = Policy::new_without_metadata(
            PolicyId::new("test"),
            "permit(principal, action, resource);".to_string(),
        );

        let new_content = "forbid(principal, action, resource);".to_string();
        let updated = original.with_content(new_content.clone());

        assert_eq!(updated.id(), original.id());
        assert_eq!(updated.content(), new_content);
    }
}
