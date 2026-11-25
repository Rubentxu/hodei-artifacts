//! HSP-314: the delivered Cedar schemas accept the canonical artifact HRN.
//!
//! The investigation (and every other read of an artifact) asks the resource
//! gate with the canonical HRN of the artifact, whose id carries a `/`
//! (`<repository>/<algorithm>-<hex>`). The HRN mapper derives the parent of
//! such an id and gives it the SAME entity type as the child, so a schema
//! that does not let `Artifacts::Artifact` be a member of itself makes Cedar
//! reject the entity set, and the gate answers `Indeterminate` (a 403).
//!
//! The test loads the schema and the policies from the files the deployment
//! ships (not a test schema) and drives the same use case and the same call
//! that `CedarReadAuthorizer` makes, with no service and no port.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use cedar_policy::{Entities, Schema};
use hodei_artifacts_api::actions;
use hodei_artifacts_api::authorization::{AuthorizeResourceUseCase, SharedAuthorizer};
use hodei_artifacts_api::trust::hrn::artifact_hrn;
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, AuthorizationOutcome, Credentials, DenyReason,
    PrincipalAuthenticator, PrincipalClaims, PrincipalId, VerifiedContext, verify_principal,
};
use hodei_evidence::ContentDigest;

struct Authenticator(&'static str);

#[async_trait]
impl PrincipalAuthenticator for Authenticator {
    async fn authenticate(
        &self,
        _credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::acme:User/{}", self.0)).unwrap(),
            AccountId::new("acme").unwrap(),
        ))
    }
}

async fn context(user: &'static str) -> VerifiedContext {
    VerifiedContext::from_principal(
        verify_principal(&Authenticator(user), &Credentials::bearer("token"))
            .await
            .unwrap(),
    )
}

fn delivered(path: &str) -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// The gate of a deployment, built from the delivered files exactly as the
/// bootstrap builds it (schema-validated).
fn gate(directory: &str, extra_policy: &str) -> AuthorizeResourceUseCase {
    let schema = Schema::from_json_str(&delivered(&format!("{directory}/schema.json")))
        .expect("the delivered schema parses");
    // The community policy is a template: fill its placeholders as the
    // quickstart does.
    let policies = delivered(&format!("{directory}/policies.cedar"))
        .replace("__ACCOUNT__", "acme")
        .replace("__USER__", "alice");
    let cedar = CedarAuthorizer::from_policies(
        &format!("{policies}\n{extra_policy}"),
        Entities::empty(),
        Arc::new(HrnCedarMapper),
        schema,
    )
    .expect("the delivered bundle is valid");
    AuthorizeResourceUseCase::new(Arc::new(SharedAuthorizer::new(Arc::new(cedar))))
}

fn subject() -> kernel::Hrn {
    let digest = ContentDigest::sha256("a".repeat(64));
    let hrn = artifact_hrn("global", "acme", "repo", &digest);
    assert!(hrn.resource_id().contains('/'), "the id must nest");
    hrn
}

#[tokio::test]
async fn the_example_bundle_decides_on_a_canonical_artifact_hrn() {
    let gate = gate("config/examples", "");
    // Positive: the policy names alice and `Investigate`.
    assert_eq!(
        gate.execute_canonical(context("alice").await, actions::investigate(), &subject())
            .expect("canonical resource"),
        AuthorizationOutcome::Permit
    );
    // Negative: another principal has no matching policy; it must be a
    // decision (`Deny`), never an `Indeterminate` evaluation error.
    assert_eq!(
        gate.execute_canonical(context("mallory").await, actions::investigate(), &subject())
            .expect("canonical resource"),
        AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
    );
}

#[tokio::test]
async fn the_community_bundle_decides_on_a_canonical_artifact_hrn() {
    // The community bundle grants nothing on artifacts, so a grant is added
    // to have a positive case; without it the answer must be a plain deny.
    let granted = gate(
        "config/community",
        r#"permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"ReadCapsule",
            resource
        );"#,
    );
    assert_eq!(
        granted
            .execute_canonical(context("alice").await, actions::read_capsule(), &subject())
            .expect("canonical resource"),
        AuthorizationOutcome::Permit
    );
    let plain = gate("config/community", "");
    assert_eq!(
        plain
            .execute_canonical(context("alice").await, actions::read_capsule(), &subject())
            .expect("canonical resource"),
        AuthorizationOutcome::Deny(DenyReason::NoMatchingPolicy)
    );
}
