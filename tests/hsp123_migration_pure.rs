//! The pure half of HSP-123: the wire parsers, the planner and the
//! inventory aggregation, verified against documents shaped like the
//! ones Nexus and Artifactory actually answer with.

use serde_json::json;

use hodei_artifacts_api::migration::{
    AssetPlan, DestinationInfo, SourceAsset, SourceRepository, artifactory_info_to_asset,
    artifactory_repositories_to_modalities, build_inventory, destination_of, importable_format,
    mode_accepts_migration, nexus_items_to_assets, nexus_repositories_to_modalities, plan_assets,
};

fn asset(repository: &str, format: &str, path: &str, sha256: Option<&str>) -> SourceAsset {
    SourceAsset {
        repository: repository.to_string(),
        format: format.to_string(),
        path: path.to_string(),
        download_url: format!("http://origin.invalid/{repository}/{path}"),
        sha1: None,
        sha256: sha256.map(str::to_string),
        size_bytes: path.len() as u64,
    }
}

fn destination(repository: &str, format: &str, mode: &str, allowed: bool) -> DestinationInfo {
    DestinationInfo {
        repository: repository.to_string(),
        format: format.to_string(),
        mode: mode.to_string(),
        accepts_writes: mode == "hosted",
        upload_allowed: allowed,
    }
}

#[test]
fn the_origin_documents_become_assets_with_their_declared_checksums() {
    let items = vec![json!({
        "repository": "maven-releases",
        "format": "maven2",
        "assets": [{
            "path": "com/example/demo/1.0/demo-1.0.jar",
            "downloadUrl": "http://origin/repository/maven-releases/com/example/demo/1.0/demo-1.0.jar",
            "checksum": {"sha1": "AA", "sha256": "BB"},
            "size": 42,
        }],
    })];
    let found = nexus_items_to_assets(&items);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].repository, "maven-releases");
    assert_eq!(found[0].format, "maven2");
    assert_eq!(found[0].size_bytes, 42);
    assert_eq!(found[0].sha1.as_deref(), Some("aa"));
    assert_eq!(found[0].sha256.as_deref(), Some("bb"));

    let children = json!({"key": "libs", "packageType": "maven", "type": "local"});
    let modalities = artifactory_repositories_to_modalities(&[children]);
    assert_eq!(modalities[0].name, "libs");
    assert_eq!(modalities[0].mode, "local");

    let info = json!({
        "downloadUri": "http://origin/libs/com/example/a-1.0.jar",
        "size": 7,
        "checksums": {"sha1": "CC", "sha256": "DD"},
    });
    let asset = artifactory_info_to_asset("libs", "com/example/a-1.0.jar", &info);
    assert_eq!(asset.sha256.as_deref(), Some("dd"));
    assert_eq!(asset.size_bytes, 7);
}

#[test]
fn only_path_resolved_formats_are_importable_and_the_rest_says_why() {
    assert_eq!(importable_format("maven2"), Some("maven2"));
    assert_eq!(importable_format("maven"), Some("maven2"));
    assert_eq!(importable_format("generic"), Some("raw"));
    assert_eq!(importable_format("npm"), None);
    let reason = hodei_artifacts_api::migration::incompatibility_reason("docker");
    assert!(reason.contains("docker"), "{reason}");
    assert!(mode_accepts_migration("hosted"));
    assert!(mode_accepts_migration("local"));
    assert!(!mode_accepts_migration("proxy"));
    assert!(!mode_accepts_migration("remote"));
    assert!(!mode_accepts_migration("group"));
    assert!(!mode_accepts_migration("virtual"));
}

#[test]
fn the_plan_imports_what_fits_and_names_the_reason_for_what_does_not() {
    let jar = asset(
        "maven-releases",
        "maven2",
        "com/example/demo-1.0.jar",
        Some("aa"),
    );
    let wheel = asset("pypi-local", "pypi", "demo/demo-1.0.tar.gz", None);
    let assets = [jar.clone(), wheel];
    let destinations = [
        destination("maven-releases", "maven2", "hosted", true),
        destination("pypi-local", "pypi", "hosted", true),
    ];
    let plans = plan_assets(&assets, &destinations);
    assert!(
        matches!(&plans[0], AssetPlan::Import { destination } if destination == "maven-releases")
    );
    let AssetPlan::Skip { reason } = &plans[1] else {
        panic!("a format without a path surface must not be planned");
    };
    assert!(reason.contains("pypi"), "{reason}");

    // The rest of the gates, one by one.
    let no_destination = plan_assets(std::slice::from_ref(&jar), &[]);
    let AssetPlan::Skip { reason } = &no_destination[0] else {
        panic!("an unknown destination is not importable");
    };
    assert!(reason.contains("does not exist"), "{reason}");

    let group = plan_assets(
        std::slice::from_ref(&jar),
        &[destination("maven-releases", "maven2", "group", true)],
    );
    let AssetPlan::Skip { reason } = &group[0] else {
        panic!("a group stores nothing to write into");
    };
    assert!(reason.contains("group"), "{reason}");

    let wrong_format = plan_assets(
        std::slice::from_ref(&jar),
        &[destination("maven-releases", "raw", "hosted", true)],
    );
    let AssetPlan::Skip { reason } = &wrong_format[0] else {
        panic!("raw bytes are not maven content");
    };
    assert!(reason.contains("not maven2"), "{reason}");

    let forbidden = plan_assets(
        std::slice::from_ref(&jar),
        &[destination("maven-releases", "maven2", "hosted", false)],
    );
    let AssetPlan::Skip { reason } = &forbidden[0] else {
        panic!("no upload permission means no upload");
    };
    assert!(reason.contains("may not upload"), "{reason}");
}

#[test]
fn the_destination_maps_where_said_and_names_the_origin_repository_otherwise() {
    let requested = Some(vec![json!({
        "source_repository": "maven-releases",
        "repository": "maven-hosted",
    })]);
    assert_eq!(
        destination_of(&requested, "maven-releases"),
        "maven-hosted",
        "the request said where"
    );
    assert_eq!(
        destination_of(&requested, "raw-hosted"),
        "raw-hosted",
        "what the request does not say keeps its own name"
    );
    assert_eq!(destination_of(&None, "anything"), "anything");
}

#[test]
fn the_inventory_adds_up_and_spells_out_every_difference() {
    let jar = asset(
        "maven-releases",
        "maven2",
        "com/example/demo-1.0.jar",
        Some("aa"),
    );
    let wheel = asset("pypi-local", "pypi", "demo/demo-1.0.tar.gz", None);
    let docker = asset(
        "docker-hosted",
        "docker",
        "library/nginx/1.0/manifest",
        None,
    );
    let cached = asset("npm-proxy", "npm", "demo/-/demo-1.0.0.tgz", None);
    let assets = [jar.clone(), wheel, docker, cached];
    let destinations = [
        destination("maven-releases", "maven2", "hosted", true),
        destination("pypi-local", "pypi", "hosted", true),
        destination("docker-hosted", "raw", "hosted", true),
    ];
    let plans = plan_assets(&assets, &destinations);
    let modalities = [
        SourceRepository {
            name: "npm-proxy".to_string(),
            format: "npm".to_string(),
            mode: "proxy".to_string(),
        },
        SourceRepository {
            name: "maven-releases".to_string(),
            format: "maven2".to_string(),
            mode: "hosted".to_string(),
        },
    ];
    let report = build_inventory(&assets, &plans, &modalities, &destinations);

    assert_eq!(report["totals"]["assets"], 4);
    assert_eq!(report["totals"]["planned"], 1);
    let repositories = report["repositories"].as_array().expect("a list");
    let maven = repositories
        .iter()
        .find(|entry| entry["repository"] == "maven-releases")
        .expect("maven appears");
    assert_eq!(maven["planned"], 1);
    assert_eq!(maven["mode"], "hosted", "the modality came from the origin");
    let incompatibilities = report["incompatibilities"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| {
            format!(
                "{} {}",
                entry["repository"].as_str().unwrap_or_default(),
                entry["reason"].as_str().unwrap_or_default(),
            )
        })
        .collect::<Vec<String>>()
        .join("\n");
    assert!(
        incompatibilities.contains("the format pypi is resolved"),
        "{incompatibilities}"
    );
    assert!(
        incompatibilities.contains("npm-proxy") && incompatibilities.contains("proxy"),
        "a proxy is an explicit difference even when it holds cached assets: {incompatibilities}"
    );
    assert!(
        incompatibilities.contains("the origin repository is a proxy"),
        "the modality difference is spelled out per repository: {incompatibilities}"
    );
}

#[test]
fn the_nexus_modality_document_reads_into_name_format_and_type() {
    let entries = vec![
        json!({"name": "maven-releases", "format": "maven2", "type": "hosted"}),
        json!({"name": "npm-proxy", "format": "npm", "type": "proxy"}),
    ];
    let found = nexus_repositories_to_modalities(&entries);
    assert_eq!(found.len(), 2);
    assert_eq!(found[1].mode, "proxy");
}
