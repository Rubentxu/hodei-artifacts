//! The catalog search over the database.
//!
//! The search has no engine: the rows of `catalog_asset` are what is
//! searched, and the indexes of the database are what make them findable.
//! A separate index would force the catalog to be copied somewhere else,
//! and a copy is where an index starts to lie.
//!
//! # One repository per query
//!
//! The search asks **per repository**, not per account:
//!
//! ```text
//! SELECT ... FROM catalog_asset
//! WHERE account = $account AND repository = $repository AND ...
//! LIMIT <window>
//! ```
//!
//! The account and the repository are the first two columns of the
//! `(account, repository, asset_path)` index, so the index serves the
//! query. The repositories the principal may read are asked **one by one**
//! and their results are joined in the adapter. It is not a whim: it is
//! what avoids the two traps of the engine.
//!
//! - An `IN` over an array in the outer filter sometimes returns zero
//!   rows: it was measured with 100 interleaved rounds over the same
//!   database, and 28 of them came back empty.
//! - A subquery in the outer filter (`IN (SELECT VALUE ...)`) is resolved
//!   by the engine **per row**: the plan says `Fallback: Unsupported
//!   subquery`, and counting 2000 rows took 2.9 s.
//!
//! With one repository per query there is neither, and the attribute
//! filter needs no subquery either: the format, the mode and the channel
//! are already in the repository descriptor, and are filtered here.
//!
//! # The order
//!
//! The query carries **no `ORDER BY`**, and that is why the engine does
//! not sort: sorting would have to bring **all** the matching rows before
//! returning the first twenty (the plan says `MemoryOrderedLimit`, and it
//! was measured: 500 rows of an index cost 170 ms in sorting alone, and
//! in an account of a hundred thousand assets that would be seconds).
//!
//! Without `ORDER BY`, the rows come out in the order of the index, and
//! **that is the order of the page**: the walk and the repository filter
//! come out in the order of the path, and a filter with an index of its
//! own comes out in the order of that index, which is the order the
//! client asked for. Every index ends in the path, and the path is
//! unique inside a repository, so the order is total: a page neither
//! repeats nor loses a result between pages.
//!
//! A version on its own is not a filter with an index of its own: the
//! index of the coordinates asks for the name of the package first. The
//! query that asks only for a version is served by the index of the
//! path, and its rows come out in the order of the path.
//!
//! The repositories are asked one by one and their windows are joined in
//! the order of the repository name, so the joined order is the order of
//! the whole account. The window that is asked for brings one row more,
//! to know whether there is more.
//!
//! # The total
//!
//! The total costs too, and for a reason that is not visible in the code:
//! the engine **materialises every row it counts** (0.3 ms per row as
//! measured), while counting through the index without touching the rows
//! costs 0.004 ms per row. The difference is the rows that have to be
//! counted, not the rows that have to be seen.
//!
//! That is why the total is counted up to [`MAX_COUNTED`]: while it fits
//! it is exact, and when it does not fit the search says there is more and
//! does not say how much. A repository that was emptied by the window
//! already has its total, and does not have to be counted again.
//!
//! The count asks for **one row more** than what is left of the cap:
//! that extra row is what tells "exactly the cap" from "more than the
//! cap", and without it the last exact total would be thrown away. And
//! the cap holds even when the window is deep: a page that starts far
//! away brings more rows than the cap, and a total above the cap is not
//! counted.
//!
//! # The trust
//!
//! The trust state is not read from `catalog_reference`: that row holds
//! what the *admission* said at publish time, and never changes again.
//! What decides it is the **grant**, and it lives in `trust_grant`. It is
//! asked for the assets of the page, because it cannot be read in the
//! same query without a union the engine does not have.
//!
//! Reading it from the ledger and not from the reference is what makes a
//! promotion or a revocation show up in the next request, without
//! rewriting anything: there is no index to update because there is no
//! second state.
//!
//! # The trusted channel only shows what is granted
//!
//! A repository served on the `trusted` channel serves only what has a
//! live grant, and the same rule holds for what is *listed*: the other
//! listings of the service filter by the same grants, and a listing that
//! did not filter would say that the content exists where the download
//! answers that it does not.
//!
//! So a trusted repository is asked for the assets of its grants, one
//! grant at a time, and what is not granted is not counted either: the
//! total of a trusted repository counts the granted assets and nothing
//! else. The total is exact while those assets fit in the window; when
//! the grants outgrow it, the search stops asking grants and says there
//! is more without saying how much.
//!
//! One grant per **digest**: the book holds a row per decision, and two
//! decisions can grant the same content. The content is served once and
//! counted once, because a page that shows it twice says that there are
//! two contents where there is one.
//!
//! And the rows are put in the order of the query, because the order of
//! the grants is the order of the decisions and not the order the client
//! sees.
//!
//! # The `LIMIT` goes as a number
//!
//! The engine cannot evaluate a bound `LIMIT` (`Found NONE but the LIMIT
//! clause must evaluate to a positive integer`). The numbers come from a
//! validated `usize`, so no text from the client is pasted into the
//! query. The offset is not given to the engine at all: the window is
//! asked for whole and the page is cut here.

use std::collections::HashSet;

use async_trait::async_trait;
use chrono::Utc;
use hodei_catalog::search::{MAX_COUNTED, RepositoryAttribute, SearchHit, SearchPage, SearchQuery};
use hodei_catalog::{
    CatalogError, CatalogSearch, RepositoryDescriptor, RepositoryFormat, ServeChannel, TrustLedger,
    TrustState,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use surrealdb::sql::Datetime;

use super::{AssetRow, RepositoryRow, SurrealCatalogStore, algorithm_of};

/// How many repositories may share a tag.
///
/// A tag lives in a repository, and the same name may be in many. The
/// limit is a frontier: a search with a thousand repositories carrying
/// the same tag is not a search.
const MAX_TAG_TARGETS: usize = 64;

/// The row of a grant.
#[derive(Debug, Clone, Deserialize)]
struct GrantRow {
    digest_algorithm: String,
    digest: String,
}

/// A grant row read for the trust state of a page: the triple plus the
/// half of the validity the row can say without the whole grant.
#[derive(Debug, Clone, Deserialize)]
struct ValidityRow {
    repository: String,
    digest_algorithm: String,
    digest: String,
    #[serde(default)]
    policy_version: Option<String>,
    #[serde(default)]
    valid_until: Option<Datetime>,
}

/// The coordinates a tag points at.
#[derive(Debug, Clone, Deserialize)]
struct TagRow {
    repository: String,
    package_name: String,
    package_version: String,
}

#[async_trait]
impl CatalogSearch for SurrealCatalogStore {
    async fn repositories(
        &self,
        principal_account: &str,
    ) -> Result<Vec<RepositoryDescriptor>, CatalogError> {
        let mut response = self
            .db
            .query(
                "SELECT account, repo_key, hrn, repo_format, repo_mode, config_version, \
                 digest_scope, config_settings, created_at, updated_at FROM catalog_repository \
                 WHERE account = $account ORDER BY repo_key",
            )
            .bind(("account", principal_account.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<RepositoryRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        rows.into_iter()
            .map(RepositoryRow::into_descriptor)
            .collect()
    }

    async fn search(
        &self,
        principal_account: &str,
        repositories: &[RepositoryDescriptor],
        query: &SearchQuery,
    ) -> Result<SearchPage, CatalogError> {
        query
            .validate()
            .map_err(|error| CatalogError::invalid(error.to_string()))?;

        // A principal that cannot read a single repository sees nothing:
        // the search is not a way around the permission.
        if repositories.is_empty() {
            return Ok(SearchPage::empty(query));
        }

        // The repositories that take part, in the order they are walked.
        // The attribute filter is applied here, with the descriptor: the
        // engine does not have to resolve it.
        let mut candidates: Vec<&RepositoryDescriptor> = repositories.iter().collect();
        candidates.sort_by(|left, right| left.repo_key.cmp(&right.repo_key));
        if let Some(named) = &query.repository {
            candidates.retain(|repository| &repository.repo_key == named);
        }
        if let Some(attribute) = &query.attribute {
            candidates.retain(|repository| the_attribute_matches(attribute, repository));
        }
        if candidates.is_empty() {
            return Ok(SearchPage::empty(query));
        }

        // The tag is resolved first: what narrows the search are its
        // coordinates, not its name.
        let targets = match &query.tag {
            Some(tag) => self.the_tag_targets(principal_account, tag).await?,
            None => Vec::new(),
        };
        if query.tag.is_some() && targets.is_empty() {
            // A tag that does not exist is not a search without results:
            // it is a search for a tag that is not there.
            return Ok(SearchPage::empty(query));
        }

        // The window that is asked for, and one row more to know whether
        // there is more.
        let window = query.offset + query.limit + 1;

        let mut collected: Vec<AssetRow> = Vec::new();
        let mut counted = 0usize;
        let mut exact = true;

        for repository in &candidates {
            // The tag, in this repository. A tag that does not point here
            // narrows nothing here: that repository does not take part.
            let target = targets
                .iter()
                .find(|target| target.repository == repository.repo_key);
            if query.tag.is_some() && target.is_none() {
                continue;
            }

            if ServeChannel::of(&repository.config) == ServeChannel::Trusted {
                // A repository served on the trusted channel shows only
                // what is granted.
                let (rows, complete) = self
                    .the_granted_rows(principal_account, repository, query, target, window)
                    .await?;
                if complete {
                    // Every grant was asked, so the rows that came out
                    // are all of them: the repository knows its total.
                    counted += rows.len();
                } else {
                    // The grants stopped being asked for with rows
                    // enough: there may be more, and the total is not
                    // counted.
                    exact = false;
                }
                collected.extend(rows);
                // Only the window of the whole account is needed: the rest
                // is behind the page, and keeping it would multiply the
                // memory by the number of repositories.
                collected.truncate(window);
                continue;
            }

            let digest = the_digest_of(query);
            let (conditions, bindings) = the_conditions(
                principal_account,
                repository,
                query,
                target,
                digest
                    .as_ref()
                    .map(|(algorithm, value)| (algorithm.as_str(), value.as_str())),
            );
            let rows = self.the_window(&conditions, &bindings, window).await?;

            if exact {
                if rows.len() < window {
                    // The repository was emptied: its total is the one of
                    // the window, and it does not have to be counted
                    // again.
                    counted += rows.len();
                } else {
                    // The repository has more than was asked for: it has
                    // to be counted, and the count has a cap.
                    let remaining = MAX_COUNTED.saturating_sub(counted);
                    if remaining == 0 {
                        exact = false;
                    } else {
                        // One row more than what is left of the cap: that
                        // extra row is what tells "exactly the cap" from
                        // "more than the cap".
                        let found = self
                            .the_count(&conditions, &bindings, remaining + 1)
                            .await?;
                        if found > remaining {
                            counted += remaining;
                            exact = false;
                        } else {
                            counted += found;
                        }
                    }
                }
            }

            collected.extend(rows);
            // Only the window of the whole account is needed: the rest is
            // behind the page, and keeping it would multiply the memory
            // by the number of repositories.
            collected.truncate(window);
        }

        // The cap holds even when the window was deep: a page that starts
        // far away brings more rows than the cap, and a total above the
        // cap is not counted.
        if counted > MAX_COUNTED {
            exact = false;
        }

        // The page: the global window, minus what is left behind. The cut
        // is on the order the rows came in, which is total: a page
        // neither repeats nor loses a result between pages.
        let start = query.offset.min(collected.len());
        let end = (query.offset + query.limit).min(collected.len());
        let more = collected.len() > query.offset + query.limit;
        let mut rows = collected[start..end].to_vec();

        // The grants of the assets of the page.
        let granted = self.the_grants(principal_account, &rows).await?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows.drain(..) {
            let asset = row.into_asset()?;
            let format = repository_format(&candidates, &asset.repository);
            // The trust comes from the grant, not from the absence of
            // findings: what has no grant is quarantined.
            let trust_state = if granted.contains(&(
                asset.repository.clone(),
                algorithm_of(&asset.digest),
                asset.digest.value.clone(),
            )) {
                TrustState::Trusted
            } else {
                TrustState::Quarantined
            };
            results.push(SearchHit {
                repository: asset.repository,
                format,
                asset_path: asset.asset_path,
                package_name: asset.coordinates.package_name,
                package_version: asset.coordinates.package_version,
                digest: asset.digest,
                size_bytes: asset.size_bytes,
                trust_state,
                updated_at: asset.updated_at,
            });
        }

        Ok(SearchPage {
            total: if exact { Some(counted) } else { None },
            more,
            limit: query.limit,
            offset: query.offset,
            results,
        })
    }
}

impl SurrealCatalogStore {
    /// The window of rows of a repository.
    ///
    /// The limit goes as a number: the engine cannot evaluate a bound
    /// `LIMIT`, and the number comes from a `usize` that cannot carry
    /// text from the client.
    async fn the_window(
        &self,
        conditions: &[String],
        bindings: &Map<String, Value>,
        window: usize,
    ) -> Result<Vec<AssetRow>, CatalogError> {
        let sql = format!(
            "SELECT account, repository, asset_path, package_name, package_version, \
             digest_algorithm, digest, size_bytes, subject, updated_at FROM catalog_asset \
             WHERE {} LIMIT {window}",
            conditions.join(" AND ")
        );
        let mut response = self
            .db
            .query(&sql)
            .bind(bindings.clone())
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<AssetRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(rows)
    }

    /// How many rows match, up to the cap.
    ///
    /// They are counted in the adapter and not with `count()` because the
    /// engine materialises every row it counts, and here all that is
    /// needed is how many there are: the cap bounds them.
    async fn the_count(
        &self,
        conditions: &[String],
        bindings: &Map<String, Value>,
        cap: usize,
    ) -> Result<usize, CatalogError> {
        let sql = format!(
            "SELECT asset_path FROM catalog_asset WHERE {} LIMIT {cap}",
            conditions.join(" AND ")
        );
        let mut response = self
            .db
            .query(&sql)
            .bind(bindings.clone())
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<Value> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(rows.len())
    }

    /// The rows of a repository in the trusted channel.
    ///
    /// Only what is granted is listed: a repository served on the
    /// trusted channel does not serve what has no live grant, and a
    /// listing that showed it would say that the content exists where
    /// the download answers that it does not.
    ///
    /// The grants are asked one by one, and each one is an exact lookup
    /// in the `(account, repository, digest_algorithm, digest)` index.
    /// The window is respected: the grants stop being asked for as soon
    /// as there are enough rows.
    ///
    /// The rows come out in the order of the query and not in the order of
    /// the grants: the grants are asked one by one, and that order is the
    /// order of the decisions, not the order the client sees. The rows
    /// are put in the order the other repositories use, so that the order
    /// of the page is the same in every repository.
    ///
    /// The second value says whether **every** grant was asked: that is
    /// what tells a total that is known from one that is not.
    async fn the_granted_rows(
        &self,
        principal_account: &str,
        repository: &RepositoryDescriptor,
        query: &SearchQuery,
        tag_target: Option<&TagRow>,
        window: usize,
    ) -> Result<(Vec<AssetRow>, bool), CatalogError> {
        let grants = self
            .the_repository_grants(principal_account, &repository.repo_key)
            .await?;
        let mut rows: Vec<AssetRow> = Vec::new();
        for grant in grants {
            // A search by digest only looks at the grant of that digest.
            if let Some(digest) = &query.digest
                && (digest.value != grant.digest || algorithm_of(digest) != grant.digest_algorithm)
            {
                continue;
            }
            let (conditions, bindings) = the_conditions(
                principal_account,
                repository,
                query,
                tag_target,
                Some((grant.digest_algorithm.as_str(), grant.digest.as_str())),
            );
            let mut found = self
                .the_window(&conditions, &bindings, window - rows.len())
                .await?;
            rows.append(&mut found);
            if rows.len() >= window {
                // The rows are put in order before they are cut: the
                // window is the window of the page, and the page is in
                // the order of the query.
                the_order_of(&mut rows, query, tag_target);
                return Ok((rows, false));
            }
        }
        the_order_of(&mut rows, query, tag_target);
        Ok((rows, true))
    }

    /// The admissible grants of a repository.
    ///
    /// Only **admissible** grants: a revoked one is revoked, an expired
    /// one has stopped being worthy of trust, and one without declared
    /// validity has never been proven. They come from the ledger of the
    /// trust, so the rule of what is live is written once and not twice.
    ///
    /// One grant per **digest**: the book holds a row per decision, and
    /// two decisions can grant the same digest. The content is served
    /// once, and it is counted once: a row that comes out twice in a
    /// page says that there are two contents where there is one.
    async fn the_repository_grants(
        &self,
        principal_account: &str,
        repository: &str,
    ) -> Result<Vec<GrantRow>, CatalogError> {
        let now = Utc::now();
        let grants = self.grants_of(principal_account, repository).await?;
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let mut rows: Vec<GrantRow> = Vec::new();
        for grant in grants {
            if !grant.is_active_at(now) {
                continue;
            }
            let algorithm = algorithm_of(&grant.digest);
            let value = grant.digest.value.clone();
            if !seen.insert((algorithm.clone(), value.clone())) {
                // Another decision granted the same content: it is the
                // same content, and it is served once.
                continue;
            }
            rows.push(GrantRow {
                digest_algorithm: algorithm,
                digest: value,
            });
        }
        Ok(rows)
    }

    /// The coordinates a tag of the account points at.
    async fn the_tag_targets(
        &self,
        principal_account: &str,
        tag: &str,
    ) -> Result<Vec<TagRow>, CatalogError> {
        let mut response = self
            .db
            .query(
                "SELECT repository, package_name, package_version FROM catalog_tag \
                 WHERE account = $account AND tag_name = $tag",
            )
            .bind(("account", principal_account.to_string()))
            .bind(("tag", tag.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<TagRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        if rows.len() > MAX_TAG_TARGETS {
            return Err(CatalogError::invalid(
                "too many repositories carry that tag to search by it",
            ));
        }
        Ok(rows)
    }

    /// The admissible grants of the assets of the page.
    ///
    /// The question is about triples (repository, algorithm, digest), and
    /// each triple is an exact lookup in the
    /// `(account, repository, digest_algorithm, digest)` index. The form
    /// with an `IN` over an array would be shorter, but the engine
    /// sometimes returns zero rows with that form, and a grant that is
    /// not read is a trust that is unknown.
    ///
    /// Only **admissible** grants are counted: a revoked one is revoked,
    /// an expired one no longer says anything about the bytes, and a row
    /// without declared validity has never been proven -- the state that
    /// is read is the current one.
    async fn the_grants(
        &self,
        principal_account: &str,
        rows: &[AssetRow],
    ) -> Result<HashSet<(String, String, String)>, CatalogError> {
        if rows.is_empty() {
            return Ok(HashSet::new());
        }
        let mut conditions = vec!["account = $account".to_string()];
        let mut bindings = Map::new();
        bindings.insert("account".to_string(), json!(principal_account));
        // The triples are joined with "or": an asset is granted if
        // **any** of the digests of the page is. With "and" the whole page
        // would be left without grants as soon as an ungranted asset
        // reached the page, and the result would be that nothing is
        // trusted.
        let mut triples = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            triples.push(format!(
                "(repository = $grant_repository_{index} AND digest_algorithm = $grant_algorithm_{index} \
                 AND digest = $grant_digest_{index})"
            ));
            bindings.insert(format!("grant_repository_{index}"), json!(row.repository));
            bindings.insert(
                format!("grant_algorithm_{index}"),
                json!(row.digest_algorithm),
            );
            bindings.insert(format!("grant_digest_{index}"), json!(row.digest));
        }
        conditions.push(format!("({})", triples.join(" OR ")));
        let sql = format!(
            "SELECT repository, digest_algorithm, digest, policy_version, valid_until \
             FROM trust_grant WHERE {} AND revoked_at IS NONE",
            conditions.join(" AND ")
        );
        let mut response = self
            .db
            .query(&sql)
            .bind(bindings)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<ValidityRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let now = Utc::now();
        Ok(rows
            .into_iter()
            // La regla de la vigencia es la del dominio; aqui se aplica
            // la mitad que la fila puede decir sin el grant entero.
            .filter(|row| {
                row.policy_version.is_some()
                    && row.valid_until.as_ref().is_some_and(|until| until.0 > now)
            })
            .map(|row| (row.repository, row.digest_algorithm, row.digest))
            .collect())
    }
}

/// The format of the repository an asset belongs to.
fn repository_format(candidates: &[&RepositoryDescriptor], repository: &str) -> RepositoryFormat {
    candidates
        .iter()
        .find(|candidate| candidate.repo_key == repository)
        .map(|candidate| candidate.format)
        .unwrap_or(RepositoryFormat::Raw)
}

/// The condition of the search, with what it binds.
///
/// The filters are bound to parameters: the text the client writes is
/// never pasted into the query.
///
/// The digest comes apart: the one the client asked for and the one of
/// the grant that is being asked for are the same condition, and in a
/// trusted repository it is always the grant that says which one.
fn the_conditions(
    principal_account: &str,
    repository: &RepositoryDescriptor,
    query: &SearchQuery,
    tag_target: Option<&TagRow>,
    digest: Option<(&str, &str)>,
) -> (Vec<String>, Map<String, Value>) {
    let mut conditions = vec![
        "account = $account".to_string(),
        "repository = $repository".to_string(),
    ];
    let mut bindings = Map::new();
    bindings.insert("account".to_string(), json!(principal_account));
    bindings.insert("repository".to_string(), json!(repository.repo_key));

    if let Some(package_name) = &query.package_name {
        conditions.push("package_name = $package_name".to_string());
        bindings.insert("package_name".to_string(), json!(package_name));
    }
    if let Some(package_version) = &query.package_version {
        conditions.push("package_version = $package_version".to_string());
        bindings.insert("package_version".to_string(), json!(package_version));
    }
    if let Some((algorithm, value)) = digest {
        conditions.push("digest_algorithm = $digest_algorithm".to_string());
        conditions.push("digest = $digest".to_string());
        bindings.insert("digest_algorithm".to_string(), json!(algorithm));
        bindings.insert("digest".to_string(), json!(value));
    }
    if let Some(target) = tag_target {
        // The tag narrows by coordinates: an asset that is not the one of
        // the tag does not come out, even if it is in the same
        // repository.
        conditions.push("package_name = $tag_package_name".to_string());
        conditions.push("package_version = $tag_package_version".to_string());
        bindings.insert("tag_package_name".to_string(), json!(target.package_name));
        bindings.insert(
            "tag_package_version".to_string(),
            json!(target.package_version),
        );
    }
    if let Some(text) = &query.text {
        // The text is looked for in the package name and in the path,
        // without case distinction. There is no full text index: this is
        // a walk over the assets of the account.
        conditions.push(
            "(string::contains(string::lowercase(package_name), string::lowercase($text)) \
             OR string::contains(string::lowercase(asset_path), string::lowercase($text)))"
                .to_string(),
        );
        bindings.insert("text".to_string(), json!(text));
    }
    if !another_filter_owns_the_order(query, tag_target, digest.is_some()) {
        // The empty path is in no asset, so the condition does not change
        // what is returned: it tells the engine to use the path index,
        // and the walk comes out in the order of the path. When there is
        // a filter with an index of its own, that one rules.
        conditions.push("asset_path > ''".to_string());
    }
    (conditions, bindings)
}

/// The digest the client asked for, with its algorithm.
fn the_digest_of(query: &SearchQuery) -> Option<(String, String)> {
    query
        .digest
        .as_ref()
        .map(|digest| (algorithm_of(digest), digest.value.clone()))
}

/// Whether a filter with an index of its own rules the order.
///
/// The digest has an index that serves it, and the engine uses it: the
/// order of its rows is the one of that index. The same goes for the
/// package name and for the tag, which narrow by the coordinates. A
/// version on its own does not: the index of the coordinates asks for the
/// name of the package first, and a query that does not give it is served
/// by the index of the path. Without a filter of its own, the walk comes
/// out in the order of the path.
fn another_filter_owns_the_order(
    query: &SearchQuery,
    tag_target: Option<&TagRow>,
    has_digest: bool,
) -> bool {
    has_digest || the_coordinates_own_the_order(query, tag_target)
}

/// Whether the index of the coordinates serves the query.
///
/// The index of the coordinates is `(account, repository, package_name,
/// package_version, asset_path)`. It serves the queries that ask for the
/// name of the package, and the ones that ask for a tag, because a tag
/// narrows by the coordinates.
fn the_coordinates_own_the_order(query: &SearchQuery, tag_target: Option<&TagRow>) -> bool {
    query.package_name.is_some() || tag_target.is_some()
}

/// The order of the rows of a repository.
///
/// It is the order of the index that serves the query: the path for the
/// walk and for the filters that do not have an index of their own, and
/// the coordinates for the ones that narrow by them.
///
/// The rows of the open channel come out of the engine in this order
/// already. The rows of the trusted channel do not: they come out of the
/// grants one by one, and that is the order of the decisions. They are
/// put in this order here, so that the order of the page is the same in
/// every repository.
fn the_order_of(rows: &mut [AssetRow], query: &SearchQuery, tag_target: Option<&TagRow>) {
    if the_coordinates_own_the_order(query, tag_target) {
        rows.sort_by(|left, right| {
            left.package_name
                .cmp(&right.package_name)
                .then_with(|| left.package_version.cmp(&right.package_version))
                .then_with(|| left.asset_path.cmp(&right.asset_path))
        });
    } else {
        rows.sort_by(|left, right| left.asset_path.cmp(&right.asset_path));
    }
}

/// Whether a repository attribute is the one being looked for.
///
/// It is read from the descriptor, which is where the catalog holds the
/// format, the mode and the channel. There is no need to ask the engine:
/// the attribute filter is a question about the repository, and the
/// repository is already at hand.
fn the_attribute_matches(
    attribute: &RepositoryAttribute,
    repository: &RepositoryDescriptor,
) -> bool {
    match attribute {
        RepositoryAttribute::Format(format) => repository.format == *format,
        RepositoryAttribute::Mode(mode) => repository.mode == *mode,
        RepositoryAttribute::Channel(channel) => ServeChannel::of(&repository.config) == *channel,
    }
}
