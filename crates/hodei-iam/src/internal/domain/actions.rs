//! Domain actions for hodei-iam
//!
//! This module defines the IAM actions that can be performed.
//! All actions implement the agnostic ActionTrait.

use kernel::{ActionTrait, domain::value_objects::ServiceName};

// ============================================================================
// CreateUser Action
// ============================================================================

/// Action for creating a new user
#[allow(dead_code)]
pub struct CreateUserAction;

impl ActionTrait for CreateUserAction {
    fn name() -> &'static str {
        "CreateUser"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::User".to_string()
    }
}

// ============================================================================
// CreateGroup Action
// ============================================================================

/// Action for creating a new group
#[allow(dead_code)]
pub struct CreateGroupAction;

impl ActionTrait for CreateGroupAction {
    fn name() -> &'static str {
        "CreateGroup"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Group".to_string()
    }
}

// ============================================================================
// DeleteUser Action
// ============================================================================

/// Action for deleting a user
#[allow(dead_code)]
pub struct DeleteUserAction;

impl ActionTrait for DeleteUserAction {
    fn name() -> &'static str {
        "DeleteUser"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::User".to_string()
    }
}

// ============================================================================
// DeleteGroup Action
// ============================================================================

/// Action for deleting a group
#[allow(dead_code)]
pub struct DeleteGroupAction;

impl ActionTrait for DeleteGroupAction {
    fn name() -> &'static str {
        "DeleteGroup"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Group".to_string()
    }
}

// ============================================================================
// AddUserToGroup Action
// ============================================================================

/// Action for adding a user to a group
#[allow(dead_code)]
pub struct AddUserToGroupAction;

impl ActionTrait for AddUserToGroupAction {
    fn name() -> &'static str {
        "AddUserToGroup"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Group".to_string()
    }
}

// ============================================================================
// RemoveUserFromGroup Action
// ============================================================================

/// Action for removing a user from a group
#[allow(dead_code)]
pub struct RemoveUserFromGroupAction;

impl ActionTrait for RemoveUserFromGroupAction {
    fn name() -> &'static str {
        "RemoveUserFromGroup"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Group".to_string()
    }
}

// ============================================================================
// Artifact Management Actions
// ============================================================================

/// Action for uploading/creating an artifact
#[allow(dead_code)]
pub struct UploadArtifactAction;

impl ActionTrait for UploadArtifactAction {
    fn name() -> &'static str {
        "UploadArtifact"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Artifact".to_string()
    }
}

/// Action for downloading an artifact
#[allow(dead_code)]
pub struct DownloadArtifactAction;

impl ActionTrait for DownloadArtifactAction {
    fn name() -> &'static str {
        "DownloadArtifact"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Artifact".to_string()
    }
}

/// Action for viewing/reading an artifact
#[allow(dead_code)]
pub struct ViewArtifactAction;

impl ActionTrait for ViewArtifactAction {
    fn name() -> &'static str {
        "ViewArtifact"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Artifact".to_string()
    }
}

/// Action for updating artifact metadata
#[allow(dead_code)]
pub struct UpdateArtifactAction;

impl ActionTrait for UpdateArtifactAction {
    fn name() -> &'static str {
        "UpdateArtifact"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Artifact".to_string()
    }
}

/// Action for deleting an artifact
#[allow(dead_code)]
pub struct DeleteArtifactAction;

impl ActionTrait for DeleteArtifactAction {
    fn name() -> &'static str {
        "DeleteArtifact"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Artifact".to_string()
    }
}

/// Action for listing artifacts
#[allow(dead_code)]
pub struct ListArtifactsAction;

impl ActionTrait for ListArtifactsAction {
    fn name() -> &'static str {
        "ListArtifacts"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Artifact".to_string()
    }
}

/// Action for sharing an artifact with others
#[allow(dead_code)]
pub struct ShareArtifactAction;

impl ActionTrait for ShareArtifactAction {
    fn name() -> &'static str {
        "ShareArtifact"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Artifact".to_string()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_user_action() {
        assert_eq!(CreateUserAction::name(), "CreateUser");
        assert_eq!(CreateUserAction::service_name().as_str(), "iam");
        assert_eq!(
            CreateUserAction::action_name(),
            "Iam::Action::\"CreateUser\""
        );
        assert_eq!(CreateUserAction::applies_to_principal(), "Iam::User");
        assert_eq!(CreateUserAction::applies_to_resource(), "Iam::User");
    }

    #[test]
    fn test_create_group_action() {
        assert_eq!(CreateGroupAction::name(), "CreateGroup");
        assert_eq!(CreateGroupAction::service_name().as_str(), "iam");
        assert_eq!(
            CreateGroupAction::action_name(),
            "Iam::Action::\"CreateGroup\""
        );
        assert_eq!(CreateGroupAction::applies_to_principal(), "Iam::User");
        assert_eq!(CreateGroupAction::applies_to_resource(), "Iam::Group");
    }

    #[test]
    fn test_delete_user_action() {
        assert_eq!(DeleteUserAction::name(), "DeleteUser");
        assert_eq!(DeleteUserAction::applies_to_principal(), "Iam::User");
        assert_eq!(DeleteUserAction::applies_to_resource(), "Iam::User");
    }

    #[test]
    fn test_add_user_to_group_action() {
        assert_eq!(AddUserToGroupAction::name(), "AddUserToGroup");
        assert_eq!(AddUserToGroupAction::applies_to_resource(), "Iam::Group");
    }

    #[test]
    fn test_upload_artifact_action() {
        assert_eq!(UploadArtifactAction::name(), "UploadArtifact");
        assert_eq!(UploadArtifactAction::service_name().as_str(), "iam");
        assert_eq!(
            UploadArtifactAction::action_name(),
            "Iam::Action::\"UploadArtifact\""
        );
        assert_eq!(UploadArtifactAction::applies_to_principal(), "Iam::User");
        assert_eq!(UploadArtifactAction::applies_to_resource(), "Iam::Artifact");
    }

    #[test]
    fn test_download_artifact_action() {
        assert_eq!(DownloadArtifactAction::name(), "DownloadArtifact");
        assert_eq!(
            DownloadArtifactAction::applies_to_resource(),
            "Iam::Artifact"
        );
    }

    #[test]
    fn test_view_artifact_action() {
        assert_eq!(ViewArtifactAction::name(), "ViewArtifact");
        assert_eq!(ViewArtifactAction::applies_to_resource(), "Iam::Artifact");
    }

    #[test]
    fn test_delete_artifact_action() {
        assert_eq!(DeleteArtifactAction::name(), "DeleteArtifact");
        assert_eq!(DeleteArtifactAction::applies_to_resource(), "Iam::Artifact");
    }
}
