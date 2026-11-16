//! The catalog search.
//!
//! The search has no engine: the index is the catalog. The rows of
//! `catalog_asset` are what is searched, and the indexes of the database
//! are what make them findable. A second index would have to be
//! synchronised with the catalog on every write, and synchronisation is
//! where an index turns into a copy that lies.
//!
//! # What is searched
//!
//! An asset, with the shape of its repository and the state of its
//! reference:
//!
//! - the repository and its format, which is what gives the asset meaning;
//! - the path, which is what the client asks for;
//! - the package name and version, which are its coordinates;
//! - the digest and the size, which are the content;
//! - the trust state, which belongs to the **relationship** between the
//!   repository and the content, not to the content.
//!
//! # The order
//!
//! The order is the one of the index that serves the query, and it is
//! **stable**: the same query returns the same page while the catalog
//! does not change, and a page neither repeats nor loses a result between
//! pages. A revocation is not a change of the catalog, but it is a change
//! of what is served: the page of a repository served on the trusted
//! channel moves when a concession is revoked, because the content that
//! was granted is not served any more.
//!
//! Which index serves it depends on the filter, and that is not an
//! accident: the walk and the repository filter come out in the order of
//! the path, and a filter with an index of its own --the package, the
//! digest, the tag-- comes out in the order of that index, which is the
//! order the client asked for. A version on its own is not one of them:
//! the index of the coordinates asks for the name of the package first,
//! and a query that gives only a version is served by the index of the
//! path. Every one of those orders ends in the path, and the path is
//! unique inside a repository, so the order is **total** within the
//! account. The adapter says which order each filter gives, and the UAT
//! writes it down.
//!
//! # The total
//!
//! The total is the number of matches, and it is counted up to
//! [`MAX_COUNTED`]. Counting what a large account holds costs what
//! walking it costs, and a total that takes too long to arrive is not a
//! total: above the cap the search says there is more and does not say
//! how much. A total that was not counted is better than a total that was
//! invented.
//!
//! # The account
//!
//! The account is not a filter that can be forgotten: it is part of the
//! query. A search belongs to an account, and the results of another one
//! do not exist for it.

use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use hodei_evidence::domain::content::DigestAlgorithm;
use serde::{Deserialize, Serialize};

use crate::model::{RepositoryFormat, RepositoryMode, TrustState};
use crate::trust::ServeChannel;

/// How many results a page brings, at most.
///
/// A high limit is a limit of memory and of database time, and a low one
/// makes the search useless for what is being looked for. The client can
/// ask for less.
pub const MAX_LIMIT: usize = 100;

/// How many results a page brings when the client does not say.
pub const DEFAULT_LIMIT: usize = 20;

/// How many results can be left behind.
///
/// A huge offset is not a page: it is a question with no useful answer.
/// The offset does not go to the engine --the window is asked for whole
/// and the page is cut here--, but the window is as deep as the offset:
/// asking for the hundred thousandth result means asking for a hundred
/// thousand rows to leave them behind.
pub const MAX_OFFSET: usize = 100_000;

/// How many matches are counted, at most.
///
/// The total of a search is a number, and a number is counted. Counting
/// the matches of a large account costs what walking it costs --the
/// engine materialises every row it counts--, and a total that takes half
/// a minute to arrive is a total that never arrives.
///
/// While it fits in this cap the total is exact. When it does not fit,
/// the search says that there is more (`more`) and the total is not
/// counted: better than an invented total or an endless wait.
pub const MAX_COUNTED: usize = 256;

/// The error of a search query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// The page size does not fit.
    BadLimit(usize),
    /// The offset does not fit.
    BadOffset(usize),
    /// A filter is empty.
    EmptyFilter(&'static str),
    /// The digest cannot be read.
    BadDigest(String),
    /// The attribute cannot be read.
    BadAttribute(String),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadLimit(limit) => {
                write!(formatter, "the limit {limit} is not a page size")
            }
            Self::BadOffset(offset) => {
                write!(formatter, "the offset {offset} is beyond the window")
            }
            Self::EmptyFilter(name) => write!(formatter, "the filter {name} is empty"),
            Self::BadDigest(value) => write!(formatter, "the digest {value} is not a digest"),
            Self::BadAttribute(value) => {
                write!(formatter, "the attribute {value} is not an attribute")
            }
        }
    }
}

impl std::error::Error for SearchError {}

/// An attribute of a repository.
///
/// These are the ones the catalog holds today: the format, the mode and
/// the channel it is served on. There are no per-asset attributes, and
/// this enumeration is closed on purpose: an attribute that does not
/// exist cannot be searched, and a filter that says nothing cannot return
/// what the client meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryAttribute {
    /// The format of the content it holds.
    Format(RepositoryFormat),
    /// How it behaves towards the outside.
    Mode(RepositoryMode),
    /// The channel its content is served on.
    Channel(ServeChannel),
}

impl RepositoryAttribute {
    /// Reads an attribute from its text: `name=value`.
    ///
    /// The text is what the client writes in the query, and an unknown
    /// attribute is not an attribute: it is a query that says nothing.
    pub fn parse(text: &str) -> Result<Self, SearchError> {
        let (name, value) = text
            .split_once('=')
            .ok_or_else(|| SearchError::BadAttribute(text.to_string()))?;
        let value = value.trim();
        match name.trim() {
            "format" => Ok(Self::Format(
                RepositoryFormat::parse(value)
                    .ok_or_else(|| SearchError::BadAttribute(text.to_string()))?,
            )),
            "mode" => Ok(Self::Mode(
                RepositoryMode::parse(value)
                    .ok_or_else(|| SearchError::BadAttribute(text.to_string()))?,
            )),
            "channel" => Ok(Self::Channel(
                ServeChannel::parse(value)
                    .ok_or_else(|| SearchError::BadAttribute(text.to_string()))?,
            )),
            _ => Err(SearchError::BadAttribute(text.to_string())),
        }
    }

    /// The name of the attribute, as the client writes it.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Format(_) => "format",
            Self::Mode(_) => "mode",
            Self::Channel(_) => "channel",
        }
    }

    /// The value of the attribute, as the client writes it.
    pub fn value(&self) -> &'static str {
        match self {
            Self::Format(format) => format.as_str(),
            Self::Mode(mode) => mode.as_str(),
            Self::Channel(channel) => channel.as_str(),
        }
    }
}

/// The query of a search.
///
/// Every filter is optional and they combine with "and": an asset comes
/// out when it meets all the ones that are set. An empty filter is an
/// error and not a filter that narrows nothing: an empty `repository=` is
/// not "every repository", it is a query that says nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    /// The repository, by its key.
    pub repository: Option<String>,
    /// The package name.
    pub package_name: Option<String>,
    /// The package version.
    pub package_version: Option<String>,
    /// The digest of the content.
    pub digest: Option<ContentDigest>,
    /// The name of a tag.
    pub tag: Option<String>,
    /// An attribute of the repository.
    pub attribute: Option<RepositoryAttribute>,
    /// A text that appears in the package name or in the path.
    pub text: Option<String>,
    /// How many results the page brings.
    pub limit: usize,
    /// How many results are left behind.
    pub offset: usize,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            repository: None,
            package_name: None,
            package_version: None,
            digest: None,
            tag: None,
            attribute: None,
            text: None,
            limit: DEFAULT_LIMIT,
            offset: 0,
        }
    }
}

impl SearchQuery {
    /// A query without filters, with the default page.
    pub fn new() -> Self {
        Self::default()
    }

    /// The query, with the filter that is put on it.
    pub fn with_repository(mut self, repository: impl Into<String>) -> Self {
        self.repository = Some(repository.into());
        self
    }

    /// The query, with the filter that is put on it.
    pub fn with_package_name(mut self, name: impl Into<String>) -> Self {
        self.package_name = Some(name.into());
        self
    }

    /// The query, with the filter that is put on it.
    pub fn with_package_version(mut self, version: impl Into<String>) -> Self {
        self.package_version = Some(version.into());
        self
    }

    /// The query, with the filter that is put on it.
    pub fn with_digest(mut self, digest: ContentDigest) -> Self {
        self.digest = Some(digest);
        self
    }

    /// The query, with the digest filter the client writes.
    ///
    /// The text is `algorithm:value`, with the value in hexadecimal. A
    /// malformed digest is not a filter: it is a query that says nothing,
    /// and a filter that narrows nothing would return the whole catalog.
    pub fn with_digest_text(mut self, text: &str) -> Result<Self, SearchError> {
        let (algorithm, value) = text
            .split_once(':')
            .ok_or_else(|| SearchError::BadDigest(text.to_string()))?;
        let algorithm = match algorithm.trim().to_ascii_lowercase().as_str() {
            "sha256" => DigestAlgorithm::Sha256,
            "sha512" => DigestAlgorithm::Sha512,
            _ => return Err(SearchError::BadDigest(text.to_string())),
        };
        let digest = ContentDigest {
            algorithm,
            value: value.trim().to_ascii_lowercase(),
        };
        if !digest.is_well_formed() {
            return Err(SearchError::BadDigest(text.to_string()));
        }
        self.digest = Some(digest);
        Ok(self)
    }

    /// The query, with the filter that is put on it.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// The query, with the filter that is put on it.
    pub fn with_attribute(mut self, attribute: RepositoryAttribute) -> Self {
        self.attribute = Some(attribute);
        self
    }

    /// The query, with the filter that is put on it.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// The query, with the size of the page.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// The query, with what is left behind.
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Checks that the query can be made.
    ///
    /// An empty filter is an error, the limit has to fit in a page, and
    /// the text has to say something.
    pub fn validate(&self) -> Result<(), SearchError> {
        if self.limit == 0 || self.limit > MAX_LIMIT {
            return Err(SearchError::BadLimit(self.limit));
        }
        if self.offset > MAX_OFFSET {
            return Err(SearchError::BadOffset(self.offset));
        }
        for (value, name) in [
            (self.repository.as_deref(), "repository"),
            (self.package_name.as_deref(), "package"),
            (self.package_version.as_deref(), "version"),
            (self.tag.as_deref(), "tag"),
            (self.text.as_deref(), "text"),
        ] {
            if let Some(value) = value
                && value.trim().is_empty()
            {
                return Err(SearchError::EmptyFilter(name));
            }
        }
        Ok(())
    }
}

/// An asset found by a search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    /// The repository.
    pub repository: String,
    /// The format of the repository.
    pub format: RepositoryFormat,
    /// The path of the asset inside the repository.
    pub asset_path: String,
    /// The package name.
    pub package_name: String,
    /// The package version.
    pub package_version: String,
    /// The digest of the content.
    pub digest: ContentDigest,
    /// The bytes the content has.
    pub size_bytes: u64,
    /// The trust state of the content in this repository.
    ///
    /// The **grant** decides it: what has a live grant is trusted, and
    /// what has none is quarantined. It is not deduced from the absence
    /// of findings: content nobody ever wrote about is not trusted, it is
    /// unknown.
    pub trust_state: TrustState,
    /// When it moved last.
    pub updated_at: DateTime<Utc>,
}

/// A page of results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPage {
    /// How many results there are in total, when they could be counted.
    ///
    /// It is `None` when there are more matches than are counted
    /// ([`MAX_COUNTED`]): the search says there is more, and does not say
    /// how much, because a total that was not counted is not a total.
    pub total: Option<usize>,
    /// Whether there are more results after the page.
    ///
    /// It is what says whether the next page has to be asked for, and it
    /// does not depend on the total: with the total uncounted, the page
    /// can be the last one and still have no total.
    pub more: bool,
    /// How many results the page brings.
    pub limit: usize,
    /// How many results are left behind.
    pub offset: usize,
    /// The results of the page.
    pub results: Vec<SearchHit>,
}

impl SearchPage {
    /// An empty page, with the numbers that were asked for.
    pub fn empty(query: &SearchQuery) -> Self {
        Self {
            total: Some(0),
            more: false,
            limit: query.limit,
            offset: query.offset,
            results: Vec::new(),
        }
    }

    /// Whether the page brings nothing.
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_hit(repository: &str, asset_path: &str) -> SearchHit {
        SearchHit {
            repository: repository.to_string(),
            format: RepositoryFormat::PyPi,
            asset_path: asset_path.to_string(),
            package_name: "labpkg".to_string(),
            package_version: "1.0".to_string(),
            digest: ContentDigest::sha256("aa".repeat(32)),
            size_bytes: 10,
            trust_state: TrustState::Quarantined,
            updated_at: DateTime::<Utc>::UNIX_EPOCH,
        }
    }

    #[test]
    fn a_query_without_filters_is_a_walk() {
        // The walk is a valid query: it is the one the package browser
        // starts with.
        assert!(SearchQuery::new().validate().is_ok());
    }

    #[test]
    fn the_limit_of_a_page_is_bounded() {
        assert_eq!(SearchQuery::new().limit, DEFAULT_LIMIT);
        assert!(SearchQuery::new().with_limit(0).validate().is_err());
        assert!(SearchQuery::new().with_limit(MAX_LIMIT).validate().is_ok());
        assert!(
            SearchQuery::new()
                .with_limit(MAX_LIMIT + 1)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn the_offset_of_a_page_is_bounded() {
        assert!(SearchQuery::new().with_offset(0).validate().is_ok());
        assert!(
            SearchQuery::new()
                .with_offset(MAX_OFFSET)
                .validate()
                .is_ok()
        );
        assert!(
            SearchQuery::new()
                .with_offset(MAX_OFFSET + 1)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn an_empty_filter_is_not_a_filter() {
        assert!(SearchQuery::new().with_repository("  ").validate().is_err());
        assert!(SearchQuery::new().with_package_name("").validate().is_err());
        assert!(SearchQuery::new().with_tag("latest").validate().is_ok());
    }

    #[test]
    fn a_hit_carries_what_the_search_found() {
        let hit = a_hit("libs", "one.whl");
        assert_eq!(hit.repository, "libs");
        assert_eq!(hit.asset_path, "one.whl");
        assert_eq!(hit.trust_state, TrustState::Quarantined);
    }

    #[test]
    fn an_empty_page_says_the_numbers_that_were_asked_for() {
        let query = SearchQuery::new().with_limit(7).with_offset(3);
        let page = SearchPage::empty(&query);
        assert_eq!(page.total, Some(0));
        assert!(!page.more);
        assert_eq!(page.limit, 7);
        assert_eq!(page.offset, 3);
        assert!(page.is_empty());
    }

    #[test]
    fn a_digest_is_read_from_its_text() {
        let hex = "ab".repeat(32);
        let query = SearchQuery::new()
            .with_digest_text(&format!("sha256:{hex}"))
            .expect("a digest");
        let digest = query.digest.expect("the digest is there");
        assert_eq!(digest.algorithm, DigestAlgorithm::Sha256);
        assert_eq!(digest.value, hex);
        for bad in ["sha256", "sha1:ab", "sha256:zz", "sha256:", "sha256:ab"] {
            assert!(
                SearchQuery::new().with_digest_text(bad).is_err(),
                "{bad:?} is not a digest"
            );
        }
    }

    #[test]
    fn a_digest_in_uppercase_is_the_same_digest() {
        let hex = "AB".repeat(32);
        let query = SearchQuery::new()
            .with_digest_text(&format!("SHA256:{hex}"))
            .expect("a digest");
        assert_eq!(query.digest.expect("the digest").value, "ab".repeat(32));
    }

    #[test]
    fn an_attribute_is_read_from_its_text() {
        let attribute = RepositoryAttribute::parse("format=pypi").expect("an attribute");
        assert_eq!(attribute.name(), "format");
        assert_eq!(attribute.value(), "pypi");
        assert_eq!(
            RepositoryAttribute::parse("channel=trusted")
                .expect("an attribute")
                .value(),
            "trusted"
        );
        assert_eq!(
            RepositoryAttribute::parse("mode=proxy")
                .expect("an attribute")
                .value(),
            "proxy"
        );
        for bad in ["format", "format=zip", "colour=red", "format="] {
            assert!(
                RepositoryAttribute::parse(bad).is_err(),
                "{bad:?} is not an attribute"
            );
        }
    }

    #[test]
    fn an_attribute_says_its_name_and_its_value() {
        let attribute = RepositoryAttribute::Format(RepositoryFormat::PyPi);
        assert_eq!(attribute.name(), "format");
        assert_eq!(attribute.value(), "pypi");
        let attribute = RepositoryAttribute::Channel(ServeChannel::TrustedOnly);
        assert_eq!(attribute.name(), "channel");
        assert_eq!(attribute.value(), "trusted");
    }
}
