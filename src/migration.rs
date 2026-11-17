//! Migration from **Nexus** and **Artifactory** through their public
//! read-only APIs (HSP-123 / HAR-29).
//!
//! The source is never written: every call here is a `GET` against the
//! documented search/storage endpoints, so a failed migration leaves the
//! origin exactly as it found it — the rollback guarantee is structural,
//! not procedural.
//!
//! The pure half lives here: format mapping, per-asset planning, the
//! inventory aggregation and the wire parsers. The effecting half —
//! HTTP and the catalog — lives in `crate::handlers::migration`.

use serde::Deserialize;
use serde_json::Value;

/// What the origin is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Nexus,
    Artifactory,
}

/// How to reach the origin. Credentials are optional: an anonymous read
/// surface works too.
#[derive(Clone, Debug, Deserialize)]
pub struct SourceConfig {
    pub kind: SourceKind,
    pub base_url: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

/// One artifact the origin serves: its repository, its format, the path
/// inside the repository, where to download it, and whatever checksums
/// the origin declared (Nexus search exposes `sha1`, Artifactory also
/// `sha256`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceAsset {
    pub repository: String,
    pub format: String,
    pub path: String,
    pub download_url: String,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub size_bytes: u64,
}

/// A repository as the origin describes it: name, format and modality
/// (`hosted`, `proxy`, `group`, or Artifactory's `local`, `remote`,
/// `federation`, `virtual`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRepository {
    pub name: String,
    pub format: String,
    pub mode: String,
}

/// The origin modality is content-bearing: a proxy or a remote is a
/// window onto someone else's bytes, and a group is a view. Only what
/// the origin itself stores can be migrated.
pub fn mode_accepts_migration(mode: &str) -> bool {
    matches!(
        mode,
        "hosted" | "local" | "default" | "mixed" | "repository"
    )
}

/// The formats Hodei can absorb path-for-path in this first slice: the
/// asset layout the origin uses **is** the layout the native client
/// resolves against. Everything else needs a derived surface (indices,
/// packuments, signed metadata) that a path-copy cannot regenerate, and
/// is reported as an explicit difference instead of imported quietly.
pub fn importable_format(format: &str) -> Option<&'static str> {
    match format {
        "maven2" | "maven" => Some("maven2"),
        "raw" | "generic" => Some("raw"),
        _ => None,
    }
}

/// Why a format is not importable. Kept honest: it says what is missing,
/// not that the format is unsupported forever.
pub fn incompatibility_reason(format: &str) -> String {
    match importable_format(format) {
        Some(_) => String::new(),
        None => format!(
            "the format {format} is resolved through a surface the hub derives, not through bare paths: the migration would import bytes the client never asks for"
        ),
    }
}

/// Where the asset may land: the repository descriptor already looked up
/// in the catalog, plus the answer to the permission question so the
/// inventory can report it without asking twice.
#[derive(Clone, Debug)]
pub struct DestinationInfo {
    pub repository: String,
    pub format: String,
    pub mode: String,
    pub accepts_writes: bool,
    pub upload_allowed: bool,
}

/// The per-asset plan: which destination, or why it is skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetPlan {
    Import { destination: String },
    Skip { reason: String },
}

pub fn plan_asset(asset: &SourceAsset, destination: Option<&DestinationInfo>) -> AssetPlan {
    let Some(mapped) = importable_format(&asset.format) else {
        return AssetPlan::Skip {
            reason: incompatibility_reason(&asset.format),
        };
    };
    let Some(destination) = destination else {
        return AssetPlan::Skip {
            reason: format!(
                "the destination repository for {} does not exist in this account",
                asset.repository
            ),
        };
    };
    if !destination.accepts_writes {
        return AssetPlan::Skip {
            reason: format!(
                "the destination {} is a {}, which stores no content of its own",
                destination.repository, destination.mode
            ),
        };
    }
    if destination.format != mapped {
        return AssetPlan::Skip {
            reason: format!(
                "the destination {} holds {}, not {}",
                destination.repository, destination.format, mapped
            ),
        };
    }
    if !destination.upload_allowed {
        return AssetPlan::Skip {
            reason: format!("the caller may not upload to {}", destination.repository),
        };
    }
    AssetPlan::Import {
        destination: destination.repository.clone(),
    }
}

pub fn plan_assets(assets: &[SourceAsset], destinations: &[DestinationInfo]) -> Vec<AssetPlan> {
    assets
        .iter()
        .map(|asset| {
            plan_asset(
                asset,
                destinations
                    .iter()
                    .find(|destination| destination.repository == asset.repository),
            )
        })
        .collect()
}

/// The report that answers acceptance criterion 1: totals, per
/// repository, with the modality differences made explicit. Destinations
/// are matched by repository key unless the request said otherwise; an
/// origin repository whose content is not importable still appears, with
/// the reason attached.
pub fn build_inventory(
    assets: &[SourceAsset],
    plans: &[AssetPlan],
    origin_repositories: &[SourceRepository],
    destinations: &[DestinationInfo],
) -> Value {
    let mut repository: Value = Value::Object(Default::default());
    let mut incompatibilities: Vec<Value> = Vec::new();
    let mut total_assets = 0usize;
    let mut total_bytes = 0u64;

    for (asset, plan) in assets.iter().zip(plans) {
        let entry = repository
            .as_object_mut()
            .expect("the accumulator is an object")
            .entry(asset.repository.clone())
            .or_insert_with(|| {
                let origin = origin_repositories
                    .iter()
                    .find(|origin| origin.name == asset.repository);
                serde_json::json!({
                    "repository": asset.repository,
                    "format": asset.format,
                    "mode": origin.map(|origin| origin.mode.clone()).unwrap_or_default(),
                    "destination": asset.repository,
                    "assets": 0,
                    "bytes": 0,
                    "planned": 0,
                    "planned_bytes": 0,
                    "skipped": 0,
                })
            });
        let object = entry.as_object_mut().expect("the entry is an object");
        object["assets"] = Value::from(object["assets"].as_u64().unwrap_or(0) + 1);
        object["bytes"] = Value::from(object["bytes"].as_u64().unwrap_or(0) + asset.size_bytes);
        total_assets += 1;
        total_bytes += asset.size_bytes;
        match plan {
            AssetPlan::Import { destination } => {
                object["destination"] = Value::String(destination.clone());
                object["planned"] = Value::from(object["planned"].as_u64().unwrap_or(0) + 1);
                object["planned_bytes"] =
                    Value::from(object["planned_bytes"].as_u64().unwrap_or(0) + asset.size_bytes);
            }
            AssetPlan::Skip { reason } => {
                object["skipped"] = Value::from(object["skipped"].as_u64().unwrap_or(0) + 1);
                incompatibilities.push(serde_json::json!({
                    "repository": asset.repository,
                    "path": asset.path,
                    "reason": reason,
                }));
            }
        }
    }

    // The modality is a per-repository difference and belongs in the
    // report whether the repository has assets or not: a proxy of a
    // group says so empty, and a proxy holding cached bytes says so
    // too, because its assets sit on top of the same lie.
    for origin in origin_repositories {
        if mode_accepts_migration(&origin.mode) {
            continue;
        }
        incompatibilities.push(serde_json::json!({
            "repository": origin.name,
            "format": origin.format,
            "reason": format!(
                "the origin repository is a {}, which is a view onto other bytes, not stored content",
                origin.mode
            ),
        }));
    }

    // And a destination that exists but holds nothing matched.
    for destination in destinations {
        if repository
            .get(&destination.repository)
            .is_some_and(|entry| entry["assets"].as_u64().unwrap_or(0) > 0)
        {
            continue;
        }
        repository
            .as_object_mut()
            .expect("the accumulator is an object")
            .entry(destination.repository.clone())
            .or_insert_with(|| {
                serde_json::json!({
                    "repository": destination.repository,
                    "format": destination.format,
                    "mode": destination.mode,
                    "destination": destination.repository,
                    "assets": 0,
                    "bytes": 0,
                    "planned": 0,
                    "planned_bytes": 0,
                    "skipped": 0,
                })
            });
    }

    let mut repositories: Vec<(String, Value)> = repository
        .as_object()
        .expect("the accumulator is an object")
        .iter()
        .map(|(key, entry)| {
            let mut entry = entry.clone();
            if entry["format"].as_str().is_none_or(str::is_empty) {
                entry["format"] = Value::String(String::new());
            }
            (key.clone(), entry)
        })
        .collect();
    repositories.sort_by(|left, right| left.0.cmp(&right.0));

    serde_json::json!({
        "totals": {
            "assets": total_assets,
            "bytes": total_bytes,
            "planned": plans.iter().filter(|plan| matches!(plan, AssetPlan::Import { .. })).count(),
            "planned_bytes": assets
                .iter()
                .zip(plans)
                .filter(|(_, plan)| matches!(plan, AssetPlan::Import { .. }))
                .map(|(asset, _)| asset.size_bytes)
                .sum::<u64>(),
        },
        "repositories": repositories
            .into_iter()
            .map(|(_, entry)| entry)
            .collect::<Vec<Value>>(),
        "incompatibilities": incompatibilities,
    })
}

/// An item of the Nexus 3 search API, narrowed to what matters.
pub fn nexus_items_to_assets(items: &[Value]) -> Vec<SourceAsset> {
    let mut found = Vec::new();
    for item in items {
        let repository = string_of(item, "repository");
        let format = string_of(item, "format");
        for asset in item
            .get("assets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let checksums = asset.get("checksum").cloned().unwrap_or(Value::Null);
            found.push(SourceAsset {
                repository: repository.clone(),
                format: format.clone(),
                path: string_of(asset, "path"),
                download_url: string_of(asset, "downloadUrl"),
                sha1: optional_string_of(&checksums, "sha1"),
                sha256: optional_string_of(&checksums, "sha256"),
                size_bytes: asset
                    .get("size")
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
            });
        }
    }
    found
}

/// The repository list of the Nexus 3 REST API: modality is `type`.
pub fn nexus_repositories_to_modalities(body: &[Value]) -> Vec<SourceRepository> {
    body.iter()
        .map(|entry| SourceRepository {
            name: string_of(entry, "name"),
            format: string_of(entry, "format"),
            mode: string_of(entry, "type"),
        })
        .collect()
}

/// The repository list of Artifactory: modality is `type` (`local`,
/// `remote`, `federation`, `virtual`).
pub fn artifactory_repositories_to_modalities(body: &[Value]) -> Vec<SourceRepository> {
    body.iter()
        .map(|entry| SourceRepository {
            name: string_of(entry, "key"),
            format: string_of(entry, "packageType"),
            mode: string_of(entry, "type"),
        })
        .collect()
}

/// The storage info document of one Artifactory file.
pub fn artifactory_info_to_asset(key: &str, path: &str, body: &Value) -> SourceAsset {
    let checksums = body.get("checksums").cloned().unwrap_or(Value::Null);
    SourceAsset {
        repository: key.to_string(),
        format: string_of(body, "packageType"),
        path: path.to_string(),
        download_url: string_of(body, "downloadUri"),
        sha1: optional_string_of(&checksums, "sha1"),
        sha256: optional_string_of(&checksums, "sha256"),
        size_bytes: body.get("size").and_then(Value::as_u64).unwrap_or_default(),
    }
}

fn string_of(body: &Value, key: &str) -> String {
    body.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn optional_string_of(body: &Value, key: &str) -> Option<String> {
    body.get(key)
        .and_then(Value::as_str)
        .map(|value| value.to_lowercase())
}

/// The default destination of an origin repository: its own name, which
/// is the mapping every migration tool pretends to need.
pub fn destination_of(requested: &Option<Vec<Value>>, source_repository: &str) -> String {
    requested
        .as_ref()
        .and_then(|entries| {
            entries.iter().find_map(|entry| {
                let matches = entry.get("source_repository")?.as_str()? == source_repository;
                if matches {
                    entry.get("repository")?.as_str().map(str::to_string)
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| source_repository.to_string())
}

// The unit tests of this module live in `tests/hsp123_migration_pure.rs`.
