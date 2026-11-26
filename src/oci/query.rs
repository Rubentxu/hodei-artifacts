//! The two questions of the OCI surface that are not content: **which
//! tags exist** and **what hangs off this manifest**.
//!
//! Both are reads of the catalog, and both speak the dialect the
//! protocol specifies: the tag list with its pagination, and the
//! referrers as an image index.
//!
//! And both have a rule that looks like a detail and is not: **they do not
//! say 404**. A `subject` that has no referrers is an empty list,
//! and a repository without tags is an empty list. A 404 on the
//! referrers API would make a client looking for the signature of a manifest
//! have to distinguish between "there is no signature" and "there is no manifest", and the
//! protocol says that distinction does not exist.
//!
//! What is said with 404 is what does not exist: the **repository**. A
//! repository of another account does not exist for this one, and is answered with the
//! distribution's `NAME_UNKNOWN`.

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hodei_evidence::ContentDigest;
use hodei_hub::oci::{DOCKER_MANIFEST_LIST_MEDIA_TYPE, INDEX_MEDIA_TYPE};

use crate::app_state::AppState;
use crate::oci::failure::OciFailure;
use crate::oci::target::the_digest_of_reference;
use crate::oci::{the_catalog, the_content_store, the_decision, the_repository};

/// The header that says a filter has been applied.
///
/// The protocol requires it when a filter is requested: without it, a client
/// does not know whether the list is filtered or whether the registry did not understand
/// what was asked of it.
const FILTERS_APPLIED_HEADER: &str = "OCI-Filters-Applied";

/// The parameters of the tag list.
///
/// `n` is how many tags are requested, and `last` is the tag to start
/// after: the client's cursor, which is not included in the result.
#[derive(Debug, Default)]
struct TheTagsQuery {
    /// How many tags are requested. `None` is "all".
    n: Option<usize>,
    /// The tag to start after, not including it.
    last: Option<String>,
}

/// The parameters of the referrers query.
#[derive(Debug, Default)]
struct TheReferrersQuery {
    /// The artifact type to filter by.
    artifact_type: Option<String>,
}

/// The tag list of the repository.
///
/// `GET /v2/<name>/tags/list`
///
/// Tags go in lexicographic order, which the protocol defines as
/// alphanumeric order **case-insensitive**, and the page is
/// trimmed with `n` and `last`.
pub async fn the_tags(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    query: &str,
    headers: &HeaderMap,
) -> Result<Response, OciFailure> {
    let repository = the_repository(state, context, name).await?;
    the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;

    let parsed = the_tags_query(query)?;
    let (reader, _) = the_catalog(state)?;

    // A group stores no tags: it composes them from those of its members.
    // The list is the **union** of those of its members, one entry per
    // name, and **is not served partially**: if a member's list cannot
    // be obtained, the whole listing is an error, because a half list
    // looks complete and is not.
    let tags: Vec<String> = if repository.mode == hodei_catalog::RepositoryMode::Group {
        crate::oci::group::the_tags(state, context, &repository, headers)
            .await?
            .into_iter()
            .map(|(name, _member)| name)
            .collect()
    } else if repository.mode == hodei_catalog::RepositoryMode::Proxy {
        // A proxy has **no** tag rows in the catalog: its
        // tag is a **pointer** that only exists if someone already
        // asked for it, and its list lives in the **upstream**. That is why the list is
        // asked there, with the proxy repository's name: the same
        // name with which its manifests and blobs are asked for.
        //
        // And the listing **is not served partially**: if the list cannot be
        // obtained --a down upstream, a credential that cannot be
        // used-- the whole listing is an error, because an empty list
        // looks complete and is not. It is the same rule the group
        // applies to its members.
        let listed =
            crate::oci::proxy::the_resolved_tags(state, &repository, name, headers).await?;
        // The trust filter **cannot** be applied per tag: the
        // upstream's list is names, not digests, and there is no digest to
        // look at. It is the same decision the group takes with its proxy
        // members: the gate is applied to the **content**, at
        // resolution, which is where it is decided whether the bytes can be
        // served. A filter that cannot be applied is not invented.
        match listed {
            crate::oci::proxy::TheUpstreamTags::Listed(tags) => tags,
            // The upstream's scope does not cover the listing: the repository
            // **cannot enumerate**. In a group that does not bring down the listing
            // --the other members are listed anyway--, but in a direct
            // proxy there are no "others": the empty list is the only
            // honest answer, because what the upstream does not let
            // be enumerated does not exist for this repository.
            crate::oci::proxy::TheUpstreamTags::NotInScope => Vec::new(),
        }
    } else {
        let tags = reader
            .tags(context.account().as_str(), name)
            .await
            .map_err(|error| crate::oci::the_catalog_error(&error))?;

        // On the trusted channel only the tags whose **target**
        // has an active grant are listed: a tag towards an ungranted digest
        // would reveal that it exists.
        let servable = crate::trust::servable(state, context.account().as_str(), &repository)
            .await
            .map_err(the_admission_failure)?;
        tags.into_iter()
            .filter(|tag| {
                servable
                    .as_ref()
                    .is_none_or(|digests| digests.contains(&tag.coordinates.package_version))
            })
            .map(|tag| tag.tag_name)
            .collect()
    };
    let mut names: Vec<String> = tags;
    names.sort_by(|left, right| the_order(left).cmp(&the_order(right)));

    // The cursor: the list starts **after** `last`. The cursor is
    // compared with the same order as the list, so that a client that
    // paginates with a tag that no longer exists is not left halfway.
    let start = match &parsed.last {
        None => 0,
        Some(last) => names.partition_point(|name| the_order(name) <= the_order(last)),
    };

    let page: Vec<String> = match parsed.n {
        Some(n) => names[start.min(names.len())..]
            .iter()
            .take(n)
            .cloned()
            .collect(),
        None => names[start.min(names.len())..].to_vec(),
    };

    let body = serde_json::json!({
        "name": name,
        "tags": page,
    });

    let mut response = (StatusCode::OK, body.to_string()).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );

    // The `Link` says whether there is more: the paginating client follows it. Without
    // `n` there is no page, and with `n = 0` the protocol says there is no
    // `Link`.
    if let Some(n) = parsed.n
        && n > 0
        && let Some(last) = page.last()
        && start + page.len() < names.len()
    {
        let link = format!("</v2/{name}/tags/list?n={n}&last={last}>; rel=\"next\"");
        if let Ok(value) = HeaderValue::from_str(&link) {
            response.headers_mut().insert(header::LINK, value);
        }
    }
    Ok(response)
}

/// What is answered when the trust ledger cannot be read.
///
/// A listing that cannot know what is admitted lists nothing: what is
/// unknown is not served.
fn the_admission_failure(_admission: crate::trust::Admission) -> OciFailure {
    OciFailure::unavailable("the trust ledger is not available")
}

/// The referrers of a manifest.
///
/// `GET /v2/<name>/referrers/<digest>`
///
/// The response is an image index with one descriptor per referrer.
/// The descriptor carries the manifest's `artifactType` --or, if it does not
/// have one, the media type of its config-- and the manifest's
/// annotations.
pub async fn the_referrers(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    digest: &str,
    query: &str,
) -> Result<Response, OciFailure> {
    let repository = the_repository(state, context, name).await?;
    the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &repository,
    )?;

    let parsed = the_referrers_query(query)?;
    // The digest was already checked by the path split: here it is only
    // converted to the catalog digest.
    let subject = ContentDigest::sha256(the_digest_of_reference(digest)?);

    let (reader, _) = the_catalog(state)?;
    let account = context.account().as_str();
    // The referrers of a group are those of its **members**: a group
    // stores nothing, so its own do not exist. And the trust filter
    // is per member: a referrer towards an ungranted digest
    // would reveal that it exists.
    let mut referrers: Vec<hodei_catalog::ReferrerRecord> = Vec::new();
    if repository.mode == hodei_catalog::RepositoryMode::Group {
        for member in crate::oci::group::the_leaves(state, context, &repository).await? {
            let rows = reader
                .referrers(account, &member.repo_key, &subject)
                .await
                .map_err(|error| crate::oci::the_catalog_error(&error))?;
            let servable = crate::trust::servable(state, account, &member)
                .await
                .map_err(the_admission_failure)?;
            for row in rows {
                if let Some(digests) = &servable
                    && !digests.contains(&row.digest.value)
                {
                    continue;
                }
                referrers.push(row);
            }
        }
    } else {
        let rows = reader
            .referrers(account, name, &subject)
            .await
            .map_err(|error| crate::oci::the_catalog_error(&error))?;
        // A referrer is a manifest: only the one that could be served is listed.
        let servable = crate::trust::servable(state, account, &repository)
            .await
            .map_err(the_admission_failure)?;
        for row in rows {
            if let Some(digests) = &servable
                && !digests.contains(&row.digest.value)
            {
                continue;
            }
            referrers.push(row);
        }
    }

    let store = the_content_store(state)?;
    let mut manifests = Vec::with_capacity(referrers.len());
    for referrer in referrers {
        let bytes = store.get(&referrer.digest).await.map_err(|_| {
            OciFailure::unavailable("the catalog and the store disagree about a referrer")
        })?;
        let manifest: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| OciFailure::unavailable("the manifest of a referrer is not readable"))?;
        let descriptor = the_descriptor(&manifest, &referrer.digest.value, bytes.len());
        // The filter is on the descriptor's `artifactType`, which is the
        // one the protocol specifies: the manifest's, or that of its
        // config when the manifest does not carry it.
        if let Some(filter) = &parsed.artifact_type
            && descriptor.artifact_type.as_deref() != Some(filter.as_str())
        {
            continue;
        }
        manifests.push(descriptor.json);
    }

    let body = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": INDEX_MEDIA_TYPE,
        "manifests": manifests,
    });

    let mut response = (StatusCode::OK, body.to_string()).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(INDEX_MEDIA_TYPE),
    );
    // The filter is stated: without this header, a client does not know whether the
    // list is filtered or whether the registry did not understand the filter.
    if parsed.artifact_type.is_some() {
        response.headers_mut().insert(
            FILTERS_APPLIED_HEADER,
            HeaderValue::from_static("artifactType"),
        );
    }
    Ok(response)
}

/// The descriptor of a referrer.
///
/// The protocol says the descriptor carries the `artifactType` of the
/// manifest --or the media type of its config, when the
/// manifest is an image and does not carry it-- and the manifest's
/// annotations. An index without `artifactType` omits it, because an index
/// has no config.
fn the_descriptor(manifest: &serde_json::Value, digest: &str, size: usize) -> TheDescriptor {
    let media_type = manifest
        .get("mediaType")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let is_index = media_type == INDEX_MEDIA_TYPE || media_type == DOCKER_MANIFEST_LIST_MEDIA_TYPE;
    let declared = manifest
        .get("artifactType")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty());
    let artifact_type = match declared {
        Some(declared) => Some(declared.to_string()),
        // An index without `artifactType` omits it: the protocol says so.
        None if is_index => None,
        // Without a declared `artifactType`, the type is that of the
        // config. And if there is no config either, the type is
        // that of the manifest itself: the descriptor has to **say**
        // what kind of artifact it is, because a descriptor without a type cannot
        // be filtered by a client.
        None => manifest
            .get("config")
            .and_then(|config| config.get("mediaType"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| (!media_type.is_empty()).then(|| media_type.to_string())),
    };

    let mut json = serde_json::json!({
        "mediaType": media_type,
        "size": size,
        "digest": format!("sha256:{digest}"),
    });
    if let Some(artifact_type) = &artifact_type {
        json["artifactType"] = serde_json::Value::String(artifact_type.clone());
    }
    if let Some(annotations) = manifest.get("annotations")
        && annotations.is_object()
    {
        json["annotations"] = annotations.clone();
    }
    TheDescriptor {
        json,
        artifact_type,
    }
}

/// The descriptor of a referrer, and the `artifactType` it is
/// filtered by.
///
/// The filter goes on the descriptor's `artifactType`, and not on the
/// manifest's: they are the same value, but the descriptor's is the one the
/// protocol specifies, and the one the client sees.
struct TheDescriptor {
    /// The descriptor, as it goes in the response.
    json: serde_json::Value,
    /// The descriptor's `artifactType`.
    artifact_type: Option<String>,
}

/// The order of the tags.
///
/// The protocol defines it as alphanumeric order **case-
/// insensitive**, and the tie-break is the exact byte order: two
/// tags that differ only by case have to have an
/// order, and a stable order is what allows paginating without losing or
/// repeating.
fn the_order(tag: &str) -> (String, &str) {
    (tag.to_lowercase(), tag)
}

/// The parameters of the tag list.
///
/// An `n` that is not a number is a client error, and is stated with a
/// 400: the parameter is an integer, and an integer that is not one is a
/// badly written request, not a request to an operation the registry
/// cannot serve.
fn the_tags_query(query: &str) -> Result<TheTagsQuery, OciFailure> {
    let mut parsed = TheTagsQuery::default();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "n" => {
                // An empty `n` is not an `n`: the reference registry
                // ignores it and returns the whole list, and a client that
                // sends it is not asking for a page.
                if value.is_empty() {
                    continue;
                }
                let n = value.parse::<i64>().map_err(|_| {
                    OciFailure::pagination_number_invalid("the n parameter is not a number")
                })?;
                if n < 0 {
                    return Err(OciFailure::pagination_number_invalid(
                        "the n parameter is negative",
                    ));
                }
                parsed.n = Some(n as usize);
            }
            "last" => parsed.last = Some(value.into_owned()),
            _ => {}
        }
    }
    Ok(parsed)
}

/// The parameters of the referrers query.
fn the_referrers_query(query: &str) -> Result<TheReferrersQuery, OciFailure> {
    let mut parsed = TheReferrersQuery::default();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if key == "artifactType" {
            parsed.artifact_type = Some(value.into_owned());
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tag order is the one the protocol specifies: case-
    /// insensitive, and with a stable tie-break.
    #[test]
    fn the_order_of_the_tags_is_the_one_the_protocol_says() {
        let mut tags = vec!["beta".to_string(), "Alpha".to_string(), "alfa".to_string()];
        tags.sort_by(|left, right| the_order(left).cmp(&the_order(right)));
        assert_eq!(tags, vec!["alfa", "Alpha", "beta"]);
    }

    /// An `n` that is not a number is not a page, and is a client
    /// error: a 400, not a 405.
    #[test]
    fn an_n_that_is_not_a_number_is_not_a_page() {
        let error = the_tags_query("n=many").expect_err("the n is not a number");
        assert_eq!(error.the_code(), "PAGINATION_NUMBER_INVALID");
        assert_eq!(error.the_status(), axum::http::StatusCode::BAD_REQUEST);
        let error = the_tags_query("n=-1").expect_err("the n is negative");
        assert_eq!(error.the_code(), "PAGINATION_NUMBER_INVALID");
        let parsed = the_tags_query("n=0").expect("zero is a number");
        assert_eq!(parsed.n, Some(0));
    }

    /// The referrers filter is read as is.
    #[test]
    fn the_filter_of_the_referrers_is_read() {
        let parsed = the_referrers_query("artifactType=application/vnd.example.sbom.v1")
            .expect("the filter");
        assert_eq!(
            parsed.artifact_type.as_deref(),
            Some("application/vnd.example.sbom.v1")
        );
        let parsed = the_referrers_query("").expect("no filter");
        assert!(parsed.artifact_type.is_none());
    }

    /// The descriptor carries the manifest's `artifactType`, and the
    /// annotations.
    #[test]
    fn the_descriptor_carries_the_artifact_type_and_the_annotations() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
            "artifactType": "application/vnd.example.sbom.v1",
            "annotations": { "org.example.format": "json" },
        });
        let descriptor = the_descriptor(&manifest, "abc", 42);
        assert_eq!(
            descriptor.json["artifactType"].as_str(),
            Some("application/vnd.example.sbom.v1")
        );
        assert_eq!(descriptor.json["annotations"]["org.example.format"], "json");
        assert_eq!(descriptor.json["size"], 42);
        assert_eq!(descriptor.json["digest"], "sha256:abc");
        assert_eq!(
            descriptor.artifact_type.as_deref(),
            Some("application/vnd.example.sbom.v1")
        );
    }

    /// An image manifest without `artifactType` takes it from its
    /// config, and an index without `artifactType` omits it.
    #[test]
    fn the_artifact_type_falls_back_to_the_config_of_the_image() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": { "mediaType": "application/vnd.oci.image.config.v1+json" },
        });
        let descriptor = the_descriptor(&manifest, "abc", 42);
        assert_eq!(
            descriptor.json["artifactType"].as_str(),
            Some("application/vnd.oci.image.config.v1+json")
        );

        // And a manifest that says neither what it is nor what it has: the
        // descriptor states the manifest's own type, because a
        // descriptor without a type cannot be filtered by a client.
        let artifact = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
        });
        let descriptor = the_descriptor(&artifact, "abc", 42);
        assert_eq!(
            descriptor.json["artifactType"].as_str(),
            Some("application/vnd.oci.artifact.manifest.v1+json")
        );

        let index = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": [],
        });
        let descriptor = the_descriptor(&index, "abc", 42);
        assert!(
            descriptor.json.get("artifactType").is_none(),
            "un indice sin artifactType lo omite: {:?}",
            descriptor.json
        );
    }
}
