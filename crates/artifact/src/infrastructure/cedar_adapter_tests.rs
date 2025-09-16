use super::cedar_adapter::*;
use crate::domain::{
    artifact::{Artifact, ArtifactCoordinates, ArtifactId, ArtifactMetadata},
    physical_artifact::{ContentHash, PhysicalArtifact, PhysicalArtifactId},
    repository::RepositoryId,
    sbom::{Sbom, SbomFormat, SbomSignature},
};
use shared::{
    enums::{Ecosystem, HashAlgorithm},
    hrn::Hrn,
    lifecycle::Lifecycle,
    models::UserHrn,
};
use time::OffsetDateTime;

// Helper to create a test SBOM
fn create_test_sbom() -> Sbom {
    let hrn = Hrn::new("hrn:hodei:artifact:us-east-1:acme:sbom/sha256-abc123").unwrap();
    let artifact_hrn = Hrn::new("hrn:hodei:artifact:us-east-1:acme:physical-artifact/sha256-def456").unwrap();
    let creator_hrn = Hrn::new("hrn:hodei:iam:global:acme:user/sbom-generator").unwrap();
    
    let mut sbom = Sbom {
        hrn: hrn.clone(),
        physical_artifact_id: PhysicalArtifactId(artifact_hrn),
        format: SbomFormat::Spdx,
        format_version: "2.2".to_string(),
        content: b"SPDX-2.2...".to_vec(),
        signatures: Vec::new(),
        metadata: [
            ("author".to_string(), "John Doe".to_string()),
            ("tool".to_string(), "sbom-tool".to_string()),
        ].iter().cloned().collect(),
        generated_at: OffsetDateTime::now_utc(),
        generated_by: Some("sbom-tool".to_string()),
        generated_using: Some("1.0.0".to_string()),
        lifecycle: Lifecycle::new(creator_hrn),
    };
    
    // Add a test signature
    sbom.signatures.push(SbomSignature {
        signature: vec![1, 2, 3, 4],
        algorithm: "rsa-sha256".to_string(),
        key_id: Some("key-123".to_string()),
        signed_at: OffsetDateTime::now_utc(),
        expires_at: None,
    });
    
    sbom
}

// Helper to create a test physical artifact
fn create_test_physical_artifact() -> PhysicalArtifact {
    PhysicalArtifact {
        hrn: Hrn::new("hrn:hodei:artifact:us-east-1:acme:physical-artifact/sha256-def456").unwrap(),
        content_hash: ContentHash {
            algorithm: HashAlgorithm::Sha256,
            value: "def456".to_string(),
        },
        size_in_bytes: 1024,
        mime_type: "application/octet-stream".to_string(),
        storage_backend: "s3".to_string(),
        storage_key: "artifacts/def456".to_string(),
        organization_hrn: Hrn::new("hrn:hodei:iam:global:acme:organization/acme").unwrap(),
        created_at: OffsetDateTime::now_utc(),
    }
}

// Helper to create a test artifact
fn create_test_artifact() -> Artifact {
    Artifact {
        id: ArtifactId(Hrn::new("hrn:hodei:artifact:us-east-1:acme:artifact/mylib/1.0.0").unwrap()),
        repository_id: RepositoryId(
            Hrn::new("hrn:hodei:artifact:us-east-1:acme:repository/myrepo").unwrap()
        ),
        coordinates: ArtifactCoordinates {
            name: "mylib".to_string(),
            version: "1.0.0".to_string(),
            qualifier: "".to_string(),
        },
        metadata: ArtifactMetadata::default(),
        uploader_user_id: UserHrn(Hrn::new("hrn:hodei:iam:global:acme:user/uploader").unwrap()),
        artifact_type: Ecosystem::Maven,
        physical_artifact_id: PhysicalArtifactId(
            Hrn::new("hrn:hodei:artifact:us-east-1:acme:physical-artifact/sha256-def456").unwrap()
        ),
        created_at: OffsetDateTime::now_utc(),
    }
}

#[test]
fn sbom_adapter_smoke() {
    let sbom = create_test_sbom();
    let id = <Sbom as HodeiResource<EntityUid, RestrictedExpression>>::resource_id(&sbom);
    let attrs = <Sbom as HodeiResource<EntityUid, RestrictedExpression>>::resource_attributes(&sbom);
    let parents = <Sbom as HodeiResource<EntityUid, RestrictedExpression>>::resource_parents(&sbom);
    
    // Verify resource ID
    assert!(!id.to_string().is_empty());
    assert!(id.to_string().contains("sbom"));
    
    // Verify attributes
    assert_eq!(attrs.get("type").unwrap().clone().into_string().unwrap(), "sbom");
    assert_eq!(attrs.get("format").unwrap().clone().into_string().unwrap(), "Spdx");
    assert_eq!(attrs.get("format_version").unwrap().clone().into_string().unwrap(), "2.2");
    assert_eq!(attrs.get("signature_count").unwrap().clone().into_long().unwrap(), 1);
    
    // Verify metadata attributes
    assert!(attrs.get("metadata_author").is_some());
    assert_eq!(
        attrs.get("metadata_author").unwrap().clone().into_string().unwrap(), 
        "John Doe"
    );
    
    // Verify parent is the physical artifact
    assert_eq!(parents.len(), 1);
    assert!(parents[0].to_string().contains("physical-artifact"));
}

#[test]
fn physical_artifact_adapter_smoke() {
    let artifact = create_test_physical_artifact();
    let id = <PhysicalArtifact as HodeiResource<EntityUid, RestrictedExpression>>::resource_id(&artifact);
    let attrs = <PhysicalArtifact as HodeiResource<EntityUid, RestrictedExpression>>::resource_attributes(&artifact);
    let parents = <PhysicalArtifact as HodeiResource<EntityUid, RestrictedExpression>>::resource_parents(&artifact);
    
    // Verify resource ID
    assert!(!id.to_string().is_empty());
    assert!(id.to_string().contains("physical-artifact"));
    
    // Verify attributes
    assert_eq!(attrs.get("type").unwrap().clone().into_string().unwrap(), "physical_artifact");
    assert_eq!(attrs.get("mime_type").unwrap().clone().into_string().unwrap(), "application/octet-stream");
    assert_eq!(attrs.get("size_in_bytes").unwrap().clone().into_string().unwrap(), "1024");
    
    // Verify parent is the organization
    assert_eq!(parents.len(), 1);
    assert!(parents[0].to_string().contains("organization"));
}

#[test]
fn artifact_adapter_smoke() {
    let artifact = create_test_artifact();
    let id = <Artifact as HodeiResource<EntityUid, RestrictedExpression>>::resource_id(&artifact);
    let attrs = <Artifact as HodeiResource<EntityUid, RestrictedExpression>>::resource_attributes(&artifact);
    let parents = <Artifact as HodeiResource<EntityUid, RestrictedExpression>>::resource_parents(&artifact);
    
    // Verify resource ID
    assert!(!id.to_string().is_empty());
    assert!(id.to_string().contains("artifact"));
    
    // Verify attributes
    assert_eq!(attrs.get("type").unwrap().clone().into_string().unwrap(), "artifact");
    assert_eq!(attrs.get("coordinates.name").unwrap().clone().into_string().unwrap(), "mylib");
    assert_eq!(attrs.get("coordinates.version").unwrap().clone().into_string().unwrap(), "1.0.0");
    
    // Verify parent is the repository
    assert_eq!(parents.len(), 1);
    assert!(parents[0].to_string().contains("repository"));
}
