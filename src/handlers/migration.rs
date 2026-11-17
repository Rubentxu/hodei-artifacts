//! The administrative migration surface: `POST /migrations/inventory`
//! and `POST /migrations/import` (HSP-123 / HAR-29).
//!
//! Two phases, deliberately: **every** byte of the plan is downloaded,
//! hashed and checked against what the origin declared **before** a
//! single catalog row is written. A run that finds corruption stops
//! there and publishes nothing, which is also what makes the import
//! resumable: the next run re-plans, skips by digest what is already
//! present without moving bytes, and picks up where this one stopped.
//!
//! The origin only ever sees `GET`s — there is no code path here that
//! could write to it — so "rollback does not modify the source" is a
//! structural property, not a promise.

use std::collections::HashMap;

use axum::{
    Json,
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use hodei_authz::VerifiedContext;
use hodei_catalog::{
    AssetDisposition, CatalogError, PackageCoordinates, Publication, RepositoryDescriptor,
};
use hodei_evidence::domain::content::ContentDigest;
use hodei_hub::blob::CommitExpectation;
use sha1::{Digest as _, Sha1};
use sha2::Sha256;

use crate::app_state::AppState;
use crate::handlers::{
    correlation_id,
    raw::{
        RawApiError, hex_of, the_blob_error, the_catalog, the_content_store, the_context,
        the_current_asset, the_decision, the_repository_in,
    },
};
use crate::migration::{
    self, AssetPlan, DestinationInfo, SourceAsset, SourceConfig, SourceKind, SourceRepository,
    plan_assets,
};

/// The destination repositories a migration may touch are the ones the
/// raw surface serves: content addressed by path. The planner's format
/// rule is what additionally keeps maven content out of a raw
/// repository and the other way round.
const SERVED: &[hodei_catalog::RepositoryFormat] = &[
    hodei_catalog::RepositoryFormat::Raw,
    hodei_catalog::RepositoryFormat::Maven2,
];

/// How many search pages one inventory will read before it stops
/// calling it a broken pagination.
const MAX_SEARCH_PAGES: usize = 1_000;

#[derive(serde::Deserialize)]
pub struct MigrationRequest {
    pub source: SourceConfig,
    /// Optional destination mapping: `[{"source_repository":
    /// "releases", "repository": "maven-hosted"}]`. Without an entry,
    /// the origin repository maps onto the destination of the same key.
    #[serde(default)]
    pub destinations: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub dry_run: bool,
}

/// The origin, with its HTTP identity: base url, optional basic
/// credentials and the product-specific reading paths.
pub struct TheOrigin {
    kind: SourceKind,
    base: String,
    http: reqwest::Client,
    credentials: Option<(String, String)>,
}

impl TheOrigin {
    fn open(source: &SourceConfig) -> Result<Self, RawApiError> {
        let base = source.base_url.trim_end_matches('/').to_string();
        if !base.starts_with("http://") && !base.starts_with("https://") {
            return Err(RawApiError::BadRequest(
                "the source base url must be an http or https url",
            ));
        }
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .no_proxy()
            .build()
            .map_err(|_| RawApiError::Internal)?;
        let credentials = source
            .username
            .clone()
            .map(|username| (username, source.password.clone().unwrap_or_default()));
        Ok(Self {
            kind: source.kind,
            base,
            http,
            credentials,
        })
    }

    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        let mut request = self.http.get(url);
        if let Some((username, password)) = &self.credentials {
            request = request.basic_auth(username, Some(password));
        }
        request
    }

    async fn json(&self, url: &str) -> Result<serde_json::Value, RawApiError> {
        let response = self
            .get(url)
            .send()
            .await
            .map_err(|_| RawApiError::BadGateway("the origin could not be reached"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(RawApiError::BadGateway(
                "the origin answered the inventory with an error",
            ));
        }
        response
            .json::<serde_json::Value>()
            .await
            .map_err(|_| RawApiError::BadGateway("the origin answered with something unreadable"))
    }

    /// Every artifact the origin stores, across the pagination.
    pub async fn assets(&self) -> Result<Vec<SourceAsset>, RawApiError> {
        match self.kind {
            SourceKind::Nexus => self.nexus_assets().await,
            SourceKind::Artifactory => self.artifactory_assets().await,
        }
    }

    async fn nexus_assets(&self) -> Result<Vec<SourceAsset>, RawApiError> {
        let mut found = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..MAX_SEARCH_PAGES {
            let mut url = format!("{}/service/rest/v1/search?pageSize=1000", self.base);
            if let Some(token) = &token {
                url.push_str(&format!("&continuationToken={token}"));
            }
            let body = self.json(&url).await?;
            let items: Vec<serde_json::Value> = body
                .get("items")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            found.extend(migration::nexus_items_to_assets(&items));
            let next = body
                .get("continuationToken")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            match next {
                None => return Ok(found),
                Some(next) if Some(next.clone()) == token => {
                    return Err(RawApiError::BadGateway(
                        "the origin search pagination does not advance",
                    ));
                }
                Some(next) => token = Some(next),
            }
        }
        Err(RawApiError::BadGateway(
            "the origin search did not finish within the page budget",
        ))
    }

    async fn artifactory_assets(&self) -> Result<Vec<SourceAsset>, RawApiError> {
        let mut found = Vec::new();
        for repository in self.artifactory_repositories().await? {
            if !migration::mode_accepts_migration(&repository.mode)
                || !matches!(repository.mode.as_str(), "local")
            {
                continue;
            }
            let mut queue: Vec<String> = vec![String::new()];
            while let Some(directory) = queue.pop() {
                // `directory` is "" at the repository root and `/a/b`
                // below it; child uris arrive with their own leading
                // slash, so the full path is a plain concatenation.
                let url = format!(
                    "{}/api/storage/{}{}?list",
                    self.base, repository.name, directory
                );
                let body = self.json(&url).await?;
                let children: Vec<serde_json::Value> = body
                    .get("children")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for child in &children {
                    let Some(uri) = child.get("uri").and_then(serde_json::Value::as_str) else {
                        continue;
                    };
                    let full = format!("{directory}{uri}");
                    let is_folder = child
                        .get("folder")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    if is_folder {
                        queue.push(full);
                        continue;
                    }
                    let info = self
                        .json(&format!(
                            "{}/api/storage/{}{}",
                            self.base, repository.name, full
                        ))
                        .await?;
                    let mut asset = migration::artifactory_info_to_asset(
                        &repository.name,
                        full.trim_matches('/'),
                        &info,
                    );
                    asset.format = repository.format.clone();
                    found.push(asset);
                }
            }
        }
        Ok(found)
    }

    /// The repositories the origin declares, for the modality report.
    /// A product that does not answer this is still migratable by
    /// asset, so it degrades to an empty modality list, not to an
    /// error.
    pub async fn modalities(&self) -> Vec<SourceRepository> {
        match self.kind {
            SourceKind::Nexus => self
                .json(&format!("{}/service/rest/v1/repositories", self.base))
                .await
                .ok()
                .and_then(|body| body.as_array().cloned())
                .map(|entries| migration::nexus_repositories_to_modalities(&entries))
                .unwrap_or_default(),
            SourceKind::Artifactory => self.artifactory_repositories().await.unwrap_or_default(),
        }
    }

    async fn artifactory_repositories(&self) -> Result<Vec<SourceRepository>, RawApiError> {
        let body = self
            .json(&format!("{}/api/repositories", self.base))
            .await?;
        let entries: Vec<serde_json::Value> = body.as_array().cloned().unwrap_or_default();
        Ok(migration::artifactory_repositories_to_modalities(&entries))
    }

    /// Downloads one artifact into the content store, hashing while the
    /// bytes stream, and commits **only** if every checksum the origin
    /// declared holds. A verification failure leaves neither a staged
    /// file nor a blob behind.
    pub async fn fetch_and_verify(
        &self,
        state: &AppState,
        asset: &SourceAsset,
    ) -> Result<(ContentDigest, u64), RawApiError> {
        let response = self
            .get(&asset.download_url)
            .send()
            .await
            .map_err(|_| RawApiError::BadGateway("the origin could not be reached"))?;
        if !response.status().is_success() {
            return Err(RawApiError::BadGateway(
                "the origin did not serve the artifact",
            ));
        }
        let store = the_content_store(state)?;
        let mut session = store
            .begin()
            .await
            .map_err(|error| the_blob_error(&error))?;
        let mut sha256 = Sha256::new();
        let mut sha1 = Sha1::new();
        let mut length: u64 = 0;
        let mut response = response;
        loop {
            let chunk = response
                .chunk()
                .await
                .map_err(|_| RawApiError::BadGateway("the download was interrupted"))?;
            let Some(chunk) = chunk else { break };
            length += chunk.len() as u64;
            if length > state.max_body_size as u64 {
                let _ = session.abort().await;
                return Err(RawApiError::PayloadTooLarge);
            }
            sha256.update(&chunk);
            sha1.update(&chunk);
            session
                .append(&chunk)
                .await
                .map_err(|error| the_blob_error(&error))?;
        }
        let found_sha256 = hex_of(&sha256.finalize());
        let found_sha1 = hex_of(&sha1.finalize());
        if asset
            .sha256
            .as_deref()
            .is_some_and(|declared| !declared.eq_ignore_ascii_case(&found_sha256))
            || asset
                .sha1
                .as_deref()
                .is_some_and(|declared| !declared.eq_ignore_ascii_case(&found_sha1))
        {
            let _ = session.abort().await;
            return Err(RawApiError::BadGateway(
                "the content does not match the checksum the origin declared",
            ));
        }
        let digest = ContentDigest::sha256(found_sha256);
        let digest = session
            .commit(CommitExpectation { digest, length })
            .await
            .map_err(|error| the_blob_error(&error))?;
        Ok((digest, length))
    }
}

/// `POST /migrations/inventory`
///
/// The pre-flight report: bytes, formats, paths, permissions and
/// modality support, with every difference spelled out. Reads the
/// origin and the catalog; writes nothing anywhere.
pub async fn inventory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<MigrationRequest>,
) -> Result<Response, RawApiError> {
    let context = the_context(&state, &headers).await?;
    let client = TheOrigin::open(&request.source)?;
    let assets = client.assets().await?;
    let modalities = client.modalities().await;
    let (destinations, _descriptors) =
        the_destinations(&state, &context, &assets, &request.destinations).await?;
    let plans = plan_assets(&assets, &destinations);
    let report = migration::build_inventory(&assets, &plans, &modalities, &destinations);
    Ok(Json(report).into_response())
}

/// `POST /migrations/import`
///
/// Executes the plan. Idempotent: an asset whose destination already
/// holds the declared sha256 is counted as present without a single
/// byte moved.
pub async fn import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<MigrationRequest>,
) -> Result<Response, RawApiError> {
    let context = the_context(&state, &headers).await?;
    let client = TheOrigin::open(&request.source)?;
    let assets = client.assets().await?;
    let (destinations, descriptors) =
        the_destinations(&state, &context, &assets, &request.destinations).await?;
    let plans = plan_assets(&assets, &destinations);
    let correlation = correlation_id(&headers, &state.identity);

    let mut skipped: Vec<serde_json::Value> = Vec::new();
    let mut already_present: Vec<serde_json::Value> = Vec::new();
    let mut staged: Vec<(String, SourceAsset, ContentDigest, u64)> = Vec::new();
    let mut planned = 0usize;

    for (asset, plan) in assets.iter().zip(&plans) {
        let (destination, reason) = match plan {
            AssetPlan::Import { destination } => (Some(destination.clone()), None),
            AssetPlan::Skip { reason } => (None, Some(reason.clone())),
        };
        let Some(destination) = destination else {
            skipped.push(serde_json::json!({
                "repository": asset.repository,
                "path": asset.path,
                "reason": reason.expect("a skip carries its reason"),
            }));
            continue;
        };
        planned += 1;
        if request.dry_run {
            continue;
        }
        let current = the_current_asset(&state, &context, &destination, &asset.path).await?;
        // The incremental fast path: the origin declared a sha256, the
        // destination already holds exactly that content, and nothing
        // needs to move.
        if let (Some(current), Some(declared)) = (current.as_ref(), asset.sha256.as_ref())
            && declared.eq_ignore_ascii_case(&current.digest.value)
        {
            already_present.push(serde_json::json!({
                "repository": destination,
                "path": asset.path,
                "digest": current.digest.to_string(),
            }));
            continue;
        }

        // Phase one: download, hash while streaming, verify against
        // every checksum the origin declared, and commit the blob.
        let (digest, length) = client.fetch_and_verify(&state, asset).await?;
        if let Some(current) = current.as_ref()
            && current.digest != digest
        {
            // The destination holds other bytes under this path. The
            // migration never replaces published content, and because
            // this is found **before** any publish, the run can still
            // stop clean.
            return Err(RawApiError::Conflict(Box::leak(
                format!(
                    "the destination path {destination}/{} already holds another content",
                    asset.path
                )
                .into_boxed_str(),
            )));
        }
        staged.push((destination, asset.clone(), digest, length));
    }

    if request.dry_run {
        return Ok(Json(serde_json::json!({
            "dry_run": true,
            "planned": planned,
            "skipped": skipped,
            "already_present": already_present.len(),
        }))
        .into_response());
    }

    // Phase two: everything verified, now publish. Each publication is
    // its own atomic catalog commit with its audit event (the machinery
    // of HSP-97), coordinated through the same writer the content
    // surface uses, so blob references and holds behave identically.
    let (_, writer) = the_catalog(&state)?;
    let mut imported: Vec<serde_json::Value> = Vec::new();
    for (destination, asset, digest, length) in &staged {
        descriptors.get(destination).ok_or(RawApiError::Internal)?;
        let publication = Publication {
            repository: destination.clone(),
            asset_path: asset.path.clone(),
            coordinates: PackageCoordinates {
                package_name: asset.path.clone(),
                package_version: digest.value.clone(),
            },
            digest: digest.clone(),
            size_bytes: *length,
            tags: Vec::new(),
            actor: context.principal().subject().as_str().to_string(),
            correlation_id: correlation.clone(),
            overwrite: false,
            precondition: None,
            subject: None,
        };
        match writer
            .publish(context.account().as_str(), publication)
            .await
        {
            Ok(outcome) => imported.push(serde_json::json!({
                "repository": destination,
                "path": asset.path,
                "digest": outcome.reference.digest.to_string(),
                "size_bytes": outcome.reference.size_bytes,
                "created": matches!(outcome.disposition, AssetDisposition::Created),
            })),
            Err(CatalogError::Conflict { .. }) => already_present.push(serde_json::json!({
                "repository": destination,
                "path": asset.path,
                "digest": digest.to_string(),
                "note": "a concurrent publication beat this run to the path",
            })),
            Err(error) => return Err(crate::handlers::raw::the_publication_error(&error)),
        }
    }

    Ok(Json(serde_json::json!({
        "imported": imported,
        "already_present": already_present,
        "skipped": skipped,
        "totals": {
            "planned": planned,
            "imported": imported.len(),
            "already_present": already_present.len(),
            "skipped": skipped.len(),
        },
    }))
    .into_response())
}

/// Every destination repository the plan touches, resolved once: it
/// must exist in the verified account, be served by this release and
/// be a repository this surface knows how to address by path. The
/// permission answer (`upload_allowed`) is **not** a failure here: it
/// belongs to the report, because the inventory has to show a missing
/// permission without throwing.
async fn the_destinations(
    state: &AppState,
    context: &VerifiedContext,
    assets: &[SourceAsset],
    requested: &Option<Vec<serde_json::Value>>,
) -> Result<(Vec<DestinationInfo>, HashMap<String, RepositoryDescriptor>), RawApiError> {
    let mut names: Vec<String> = Vec::new();
    for asset in assets {
        let destination = migration::destination_of(requested, &asset.repository);
        if !names.contains(&destination) {
            names.push(destination);
        }
    }
    let mut infos = Vec::new();
    let mut descriptors = HashMap::new();
    for name in names {
        let Ok(descriptor) = the_repository_in(state, context, &name, SERVED).await else {
            continue;
        };
        let upload_allowed =
            the_decision(state, context, crate::actions::upload_asset(), &descriptor).is_ok();
        infos.push(DestinationInfo {
            repository: name.clone(),
            format: descriptor.format.as_str().to_string(),
            mode: descriptor.mode.as_str().to_string(),
            accepts_writes: descriptor.mode.accepts_client_writes(),
            upload_allowed,
        });
        descriptors.insert(name, descriptor);
    }
    Ok((infos, descriptors))
}
