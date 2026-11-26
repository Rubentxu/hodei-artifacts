//! The contract for vulnerability and license intelligence
//! (HSP-155 / SEC-05).
//!
//! Four things are promised and four things are checked with deterministic
//! fixtures and an oracle versioned in the repository:
//!
//! - AC1: the advisory lifecycle (creation, update, withdrawal,
//!   alias) and the boundary ranges per Maven/npm/PyPI produce the affected
//!   set and the justification the oracle states.
//! - AC2: a downed or lagging feed marks freshness and coverage, it does not delete
//!   CVEs; synchronization is idempotent and the air-gap import is
//!   verifiable by digest.
//! - AC3: a VEX statement is validated before being applied —author,
//!   subject, scope, justification— and an invalid statement does not
//!   hide the CVE. An unknown license stays unknown.
//! - AC4: re-evaluation is incremental by default and what is measured is
//!   numbers, not adjectives.

use hodei_risk::OsvAffected;
use hodei_risk::affected::{AffectedState, DependencyKind, Finding, apply_valid_vex};
use hodei_risk::feed::{FeedFailure, FeedPage, FeedStatus, RiskFeed, WatchedComponent, reassess};
use hodei_risk::license::{LicenseLedger, LicenseOrigin, SpdxList, parse_expression};
use hodei_risk::osv::read_osv;
use hodei_risk::version::explain;
use hodei_risk::vex::read_vex;

const CREATED: &str = include_str!("fixtures/intelligence-created.json");
const UPDATED: &str = include_str!("fixtures/intelligence-updated.json");
const WITHDRAWN: &str = include_str!("fixtures/intelligence-withdrawn.json");
const PYPI: &str = include_str!("fixtures/intelligence-pypi.json");

/// The oracle versioned alongside the code: the affected set and the
/// reason each boundary version must give.
const ORACLE: &str = include_str!("fixtures/intelligence-oracle.json");

fn clock() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-11-01T00:00:00Z")
        .expect("a clock")
        .with_timezone(&chrono::Utc)
}

fn affected_of(document: &str, index: usize) -> OsvAffected {
    let advisory = read_osv(document).expect("the document is read");
    advisory
        .affected
        .into_iter()
        .nth(index)
        .expect("the advisory has that package")
}

fn a_finding(component: &str) -> Finding {
    Finding {
        advisory_id: "GHSA-hodei-intel-0001".to_string(),
        component: component.to_string(),
        version: Some("1.5.0".to_string()),
        state: AffectedState::Affected,
        dependency: DependencyKind::Direct,
        vex: None,
    }
}

fn page(documents: Vec<String>) -> FeedPage {
    FeedPage {
        source: "osv".to_string(),
        fetched_at: clock(),
        documents,
    }
}

// ───────────────────────────── AC1 ─────────────────────────────

/// Creation, update, withdrawal and alias match the oracle, and
/// each decision carries the justification the oracle promises.
#[test]
fn the_lifecycle_and_the_limit_ranges_match_the_oracle() {
    let oracle: serde_json::Value = serde_json::from_str(ORACLE).expect("the oracle is read");

    // The advisory lifecycle.
    let created = read_osv(CREATED).expect("the creation is read");
    assert_eq!(
        created.cve(),
        Some("CVE-2026-22222"),
        "la alta publica el alias"
    );
    assert!(!created.is_withdrawn());
    assert_eq!(created.affected.len(), 1, "el conjunto afectado de la alta");

    let updated = read_osv(UPDATED).expect("the update is read");
    assert_eq!(
        updated.affected.len(),
        oracle["lifecycle"]["update_adds_a_package"]
            .as_u64()
            .expect("the oracle counts it") as usize,
        "la actualización añade el paquete npm sin tocar el Maven"
    );

    let withdrawn = read_osv(WITHDRAWN).expect("the withdrawal is read");
    assert!(withdrawn.is_withdrawn(), "la retirada se lee como retirada");
    assert_eq!(
        withdrawn.affected.len(),
        1,
        "y no borra lo que el advisory decía: sigue nombrando el paquete"
    );

    // The boundary ranges per ecosystem, against the oracle.
    for set in oracle["limit_ranges"]
        .as_array()
        .expect("the oracle has ranges")
    {
        let ecosystem = set["ecosystem"].as_str().expect("an ecosystem");
        let (document, index) = match ecosystem {
            "Maven" => (CREATED, 0),
            "npm" => (UPDATED, 1),
            "PyPI" => (PYPI, 0),
            other => panic!("the oracle names {other}"),
        };
        let affected = affected_of(document, index);
        assert_eq!(affected.package.ecosystem, ecosystem);

        for probe in set["probes"].as_array().expect("the oracle has probes") {
            let version = probe["version"].as_str().expect("a version");
            let expected = probe["match"].as_str().expect("a match");
            let (matched, reason) = explain(&affected, version);
            assert_eq!(
                matched.as_str(),
                expected,
                "the {ecosystem} probe {version} must answer {expected}, and said: {reason}"
            );
            for fragment in probe["reason_contains"]
                .as_array()
                .expect("the oracle gives the reason")
            {
                let fragment = fragment.as_str().expect("a fragment");
                assert!(
                    reason.contains(fragment),
                    "the reason for {ecosystem} {version} must say `{fragment}` and said: {reason}"
                );
            }
        }
    }
}

// ───────────────────────────── AC2 ─────────────────────────────

/// The lifecycle goes through the feed: update, withdrawal, out-of-order
/// pages; the downed feed marks freshness and does not delete CVEs; the
/// synchronization is idempotent.
#[test]
fn the_feed_keeps_the_cves_through_its_lifecycle_and_its_failures() {
    let mut feed = RiskFeed::new(chrono::Duration::hours(6));

    // Creation of two advisories with different aliases.
    let first = feed.apply_page(
        Ok(page(vec![CREATED.to_string(), PYPI.to_string()])),
        clock(),
    );
    assert_eq!(first.inserted, 2);
    assert_eq!(first.changed_ids.len(), 2, "lo que hay que reevaluar");
    assert_eq!(feed.coverage().canonical_vulnerabilities, 2);

    // The same page again changes nothing.
    let second = feed.apply_page(
        Ok(page(vec![CREATED.to_string(), PYPI.to_string()])),
        clock(),
    );
    assert_eq!(second.inserted, 0);
    assert_eq!(second.unchanged, 2, "idempotente");
    assert!(second.changed_ids.is_empty(), "nada que reevaluar");

    // The update of the same advisory is an update and not a creation.
    let update = feed.apply_page(Ok(page(vec![UPDATED.to_string()])), clock());
    assert_eq!((update.updated, update.inserted), (1, 0));

    // A lagging page that brings the old version does NOT undo what was seen.
    let late = feed.apply_page(Ok(page(vec![CREATED.to_string()])), clock());
    assert_eq!((late.rejected, late.updated, late.inserted), (1, 0, 0));
    assert_eq!(
        feed.by_alias("GHSA-hodei-intel-0001")
            .expect("the advisory is still there")
            .affected
            .len(),
        2,
        "la versión buena del registro es la actualizada"
    );

    // The withdrawal updates the record and KEEPS it.
    let withdrawal = feed.apply_page(Ok(page(vec![WITHDRAWN.to_string()])), clock());
    assert_eq!((withdrawal.updated, withdrawal.withdrawn_kept), (1, 1));
    let coverage = feed.coverage();
    assert_eq!(
        coverage.advisories, 2,
        "los dos advisories siguen en el feed"
    );
    assert_eq!(coverage.withdrawn, 1, "y se dice que uno se retiró");
    assert!(coverage.by_ecosystem.contains_key("PyPI"));

    // The downed feed marks freshness and does NOT delete CVEs.
    let down = feed.apply_page(
        Err(FeedFailure::Unavailable("connection refused".to_string())),
        clock() + chrono::Duration::minutes(60),
    );
    assert_eq!(
        down.freshness,
        FeedStatus::Down {
            lag_minutes: 60,
            failures: 1
        }
    );
    assert!(down.freshness.is_impaired());
    assert_eq!(feed.coverage().advisories, 2, "los CVEs siguen ahí");
    assert!(
        feed.by_alias("CVE-2026-22222").is_some(),
        "el alias sigue respondiendo con el feed caído"
    );
    assert_eq!(
        feed.status(clock() + chrono::Duration::minutes(8 * 60)),
        FeedStatus::Down {
            lag_minutes: 480,
            failures: 1
        },
        "el retraso se mide en minutos desde la última página buena"
    );
}

/// The air-gap import is verifiable: the digest rules, and repeating does not
/// duplicate.
#[test]
fn the_air_gap_import_is_verified_and_idempotent() {
    let mut source = RiskFeed::new(chrono::Duration::hours(6));
    source.apply_page(Ok(page(vec![CREATED.to_string()])), clock());
    let bundle = source.export_bundle("osv", clock());

    let mut air_gapped = RiskFeed::new(chrono::Duration::hours(6));
    let first = air_gapped.import_bundle(&bundle, clock());
    assert_eq!((first.inserted, first.rejected), (1, 0));

    let second = air_gapped.import_bundle(&bundle, clock() + chrono::Duration::minutes(1));
    assert_eq!(
        (second.inserted, second.unchanged),
        (0, 1),
        "el mismo bundle dos veces deja el mismo estado"
    );

    let mut tampered = bundle.clone();
    tampered.entries[0].document = UPDATED.to_string();
    let report = air_gapped.import_bundle(&tampered, clock());
    assert_eq!(report.rejected, 1, "el documento no coincide con su digest");
    assert_eq!(air_gapped.len(), 1, "y el registro confiable no se toca");
    assert_eq!(
        air_gapped
            .by_alias("GHSA-hodei-intel-0001")
            .expect("the advisory")
            .affected
            .len(),
        1,
        "sigue siendo la versión creada, no la alterada"
    );
}

// ───────────────────────────── AC3 ─────────────────────────────

/// A VEX statement is validated against author, subject, scope and
/// justification before being applied; an invalid one does not hide the CVE.
#[test]
fn an_invalid_vex_statement_never_hides_the_cve() {
    let trusted = ["https://example.test/security"];

    // The valid statement: recognized author, subject, scope and
    // justification.
    let mut statement = read_vex(
        include_str!("fixtures/vex-not-affected.json"),
        "CVE-2024-99999",
    )
    .expect("the fixture is read");
    statement.verified = true;
    assert_eq!(
        statement.author.as_deref(),
        Some("https://example.test/security"),
        "el autor del documento se lee"
    );
    assert!(statement.is_valid_for("CVE-2024-99999", "org.example:lib-b", Some(&trusted)));

    let mut finding = a_finding("org.example:lib-b");
    apply_valid_vex(&mut finding, &statement, "CVE-2024-99999", Some(&trusted))
        .expect("the statement is valid and applies");
    assert_eq!(finding.state, AffectedState::Unaffected);
    assert_eq!(
        finding.vex.clone().expect("the reference is there").id,
        "CVE-2024-99999"
    );

    // An author the platform does not recognize: the statement is NOT
    // applied and the finding stays there.
    let mut finding = a_finding("org.example:lib-b");
    let reasons = apply_valid_vex(
        &mut finding,
        &statement,
        "CVE-2024-99999",
        Some(&["https://elsewhere"]),
    )
    .expect_err("the author is not the one trusted");
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("not one the platform trusts"))
    );
    assert_eq!(
        finding.state,
        AffectedState::Affected,
        "a statement nobody authorized does not silence the CVE"
    );
    assert!(finding.vex.is_none(), "and it is not recorded");

    // Without justification for a status that requires it.
    let mut bare = statement.clone();
    bare.justification = None;
    let mut finding = a_finding("org.example:lib-b");
    let reasons = apply_valid_vex(&mut finding, &bare, "CVE-2024-99999", Some(&trusted))
        .expect_err("it gives no reason");
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("no justification"))
    );
    assert_eq!(finding.state, AffectedState::Affected);

    // Scope that does not cover the finding's component.
    let mut finding = a_finding("org.example:lib-maven");
    let reasons = apply_valid_vex(&mut finding, &statement, "CVE-2024-99999", Some(&trusted))
        .expect_err("it covers something else");
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("does not cover the component"))
    );
    assert_eq!(finding.state, AffectedState::Affected);

    // Subject that is not that of the queried CVE.
    let mut finding = a_finding("org.example:lib-b");
    let reasons = apply_valid_vex(&mut finding, &statement, "CVE-2020-00000", Some(&trusted))
        .expect_err("it is about another vulnerability");
    assert!(reasons.iter().any(|reason| reason.contains("is about")));
    assert_eq!(finding.state, AffectedState::Affected);

    // The fixture of an unverified "fixed" statement still does not
    // silence anything: validity does not change that rule.
    let unverified = read_vex(
        include_str!("fixtures/vex-unverified-fixed.json"),
        "CVE-2024-99999",
    )
    .expect("the fixture is read");
    assert!(!unverified.is_verified());
    let mut finding = a_finding("org.example:lib-b");
    let reasons = unverified.invalid_because("CVE-2024-99999", "org.example:lib-b", Some(&trusted));
    assert!(reasons.is_empty(), "the statement is formally valid...");
    hodei_risk::affected::apply_vex(&mut finding, &unverified);
    assert_eq!(
        finding.state,
        AffectedState::Affected,
        "...but an unverified fixed does not close it"
    );
}

/// An unknown license stays unknown: it is not invented.
#[test]
fn an_unknown_license_stays_unknown() {
    let list = SpdxList::snapshot();

    // What the expression says is recorded as it says it.
    let combined = list.fact_of("app", "MIT OR Apache-2.0 AND BSD-3-Clause");
    assert_eq!(
        combined.identifiers,
        vec!["MIT", "Apache-2.0", "BSD-3-Clause"]
    );
    assert_eq!(
        combined.expression.as_deref(),
        Some("MIT OR Apache-2.0 AND BSD-3-Clause"),
        "la expresión viaja entera"
    );

    // What the document does not say stays unsaid.
    let mut ledger = LicenseLedger::new();
    ledger.record(&list, "org.example:silent", None);
    ledger.record(&list, "org.example:noassertion", Some("NOASSERTION"));
    for component in ["org.example:silent", "org.example:noassertion"] {
        let fact = ledger
            .of(component)
            .expect("the ledger knows the component");
        assert!(fact.is_unknown(), "the license is unknown, {component}");
        assert!(fact.identifiers.is_empty(), "and it is not invented");
    }
    assert_eq!(ledger.unknown_components().len(), 2);

    // A license outside the snapshot is still named, and it is
    // said that the snapshot did not know it (it is neither deleted nor guessed).
    let odd = list.fact_of("lib", "Zed-Available");
    assert_eq!(odd.origin, LicenseOrigin::Expression);
    assert_eq!(odd.identifiers, vec!["Zed-Available"]);
    assert_eq!(odd.listed, vec![false]);

    // The deprecated one is named and flagged; it is not re-decided.
    let old = list.fact_of("lib", "GPL-2.0");
    assert_eq!(old.deprecated, vec![true]);

    // Words that are not identifiers are not read as licenses.
    assert!(parse_expression("unknown").is_empty());
}

// ───────────────────────────── AC4 ─────────────────────────────

/// Re-evaluation is incremental: the number of advisories and components
/// it touches is that of what changed. What is measured is numbers, not adjectives.
#[test]
fn the_reassessment_is_incremental_and_measured() {
    let mut feed = RiskFeed::new(chrono::Duration::hours(6));

    // The full feed: one hundred advisories of different packages.
    let documents: Vec<String> = (0..100)
        .map(|index| {
            format!(
                r#"{{"id": "GHSA-hodei-{index}.1", "modified": "2026-10-01T00:00:00Z", "affected": [
                  {{"package": {{"ecosystem": "Maven", "name": "org.example:p-{}"}},
                   "ranges": [{{"type": "ECOSYSTEM", "events": [{{"introduced": "0"}}, {{"fixed": "9.0.0"}}]}}]
                  }}] }}"#,
                index
            )
        })
        .collect();
    let loaded = std::time::Instant::now();
    let bulk = feed.apply_page(Ok(page(documents)), clock());
    let bulk_sync = loaded.elapsed();
    assert_eq!(bulk.inserted, 100);
    assert_eq!(
        bulk.changed_ids.len(),
        100,
        "la carga inicial sí lo toca todo"
    );

    // The inventory: 200 components pointing at the hundred packages.
    let components: Vec<WatchedComponent> = (0..200)
        .map(|index| WatchedComponent {
            package: format!("org.example:p-{}", index % 100),
            version: Some("1.0.0".to_string()),
            dependency: DependencyKind::Direct,
        })
        .collect();

    // A new advisory arrives; ONLY that one is re-evaluated.
    let one = r#"{
      "id": "GHSA-hodei-new.0",
      "modified": "2026-11-01T00:00:00Z",
      "affected": [
        {"package": {"ecosystem": "Maven", "name": "org.example:p-7"},
         "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "2.0.0"}]}]}
      ]
    }"#;
    let started = std::time::Instant::now();
    let report = feed.apply_page(
        Ok(FeedPage {
            source: "osv".to_string(),
            fetched_at: clock(),
            documents: vec![one.to_string()],
        }),
        clock() + chrono::Duration::seconds(1),
    );
    let page_sync = started.elapsed();
    assert_eq!(report.inserted, 1);
    assert_eq!(report.changed_ids, vec!["GHSA-hodei-new.0".to_string()]);

    let started = std::time::Instant::now();
    let round = reassess(&feed, &report.changed_ids, &components);
    let reassess_time = started.elapsed();

    // Measured: 1 advisory of 101 and 2 components of 200.
    assert_eq!(round.advisories_matched, 1);
    assert_eq!(round.advisories_held, 101);
    assert_eq!(round.components_matched, 2);
    assert_eq!(round.components_watched, 200);
    assert!(round.is_incremental());
    assert_eq!(
        round.findings.len(),
        2,
        "p-7 está dos veces en el inventario"
    );
    assert!(
        round
            .findings
            .iter()
            .all(|finding| finding.state == AffectedState::Affected),
        "1.0.0 is inside the new range"
    );

    println!(
        "HSP-155 AC4 measured: bulk_sync={bulk_sync:?} page_sync={page_sync:?} reassess={reassess_time:?} advisories=1/101 components=2/200"
    );
}
