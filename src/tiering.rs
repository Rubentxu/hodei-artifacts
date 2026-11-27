//! Read-only tiering advisor (HSP-326): the pure core and its validated
//! configuration.
//!
//! The advisor recommends, per digest, the tier a blob would live in **given the
//! facts the platform really has**, and cites every fact it used. It never
//! places anything: executing a placement is another task with its own Cedar
//! action.
//!
//! # Facts that exist and facts that do not
//!
//! | Fact | Source | Status |
//! | --- | --- | --- |
//! | `size_bytes` | content store metadata | used |
//! | `age_secs` (since last written or reused) | content store metadata | used when the store keeps a date |
//! | `references` (roots of the repository naming the digest) | catalog ledger | used |
//! | tier costs | deployment configuration | used |
//! | access frequency | none: the platform keeps no read counter | **missing** |
//! | last access | none: reads leave no trace | **missing** |
//!
//! A missing fact is never invented or defaulted: it is listed in
//! `missing_facts`, and recency means "since last written", never "since last
//! read".
//!
//! # The decision
//!
//! Tiers are ordered from hottest to coldest. A tier is *eligible* for a blob
//! when every one of its conditions holds: the blob is at least `min_age_secs`
//! old (an unknown age satisfies only a tier whose `min_age_secs` is 0), at
//! least `min_size_bytes` long, and referenced by at most `max_references`
//! roots. The recommendation is the **coldest eligible tier**; the hottest tier
//! has no conditions, so there is always one. The function is pure: same policy
//! and facts, same advice.

use serde::{Deserialize, Serialize};

/// The facts that no part of the platform records today.
pub const MISSING_FACTS: [&str; 2] = ["access_frequency", "last_access"];

const BYTES_PER_GIB: u128 = 1 << 30;

/// One tier of the configured ladder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TierConfig {
    /// The tier name, `[a-z0-9_-]{1,32}`, unique.
    pub name: String,
    /// What a GiB costs in this tier per month, in millionths of `cost_unit`.
    pub monthly_cost_micro_per_gib: u64,
    /// The blob must be at least this old (seconds since written or reused).
    #[serde(default)]
    pub min_age_secs: u64,
    /// The blob must be at least this long (bytes): tiny blobs are not worth moving.
    #[serde(default)]
    pub min_size_bytes: u64,
    /// The blob must be referenced by at most this many roots of the
    /// repository: a widely shared blob stays hot.
    #[serde(default)]
    pub max_references: Option<u32>,
}

/// The `[tiering]` section: tiers, thresholds and the cost bounds of the endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TieringConfig {
    /// A label for the cost figures (for example `USD`), 1 to 8 characters.
    #[serde(default = "default_cost_unit")]
    pub cost_unit: String,
    /// Tiers from hottest to coldest, 1 to 8.
    pub tiers: Vec<TierConfig>,
    /// The most digests one answer advises on (1 to 1000).
    #[serde(default = "default_max_digests")]
    pub max_digests: usize,
    /// The most ledger rows one request may read (1 to 100000).
    #[serde(default = "default_max_rows")]
    pub max_rows: usize,
    /// The most bytes of one answer (1024 to 4 MiB).
    #[serde(default = "default_max_response_bytes")]
    pub max_response_bytes: usize,
    /// The deadline of one request, in seconds (1 to 120).
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// The most requests served at once (1 to 32).
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
}

fn default_cost_unit() -> String {
    "USD".to_string()
}
fn default_max_digests() -> usize {
    200
}
fn default_max_rows() -> usize {
    10_000
}
fn default_max_response_bytes() -> usize {
    256 * 1024
}
fn default_timeout_secs() -> u64 {
    20
}
fn default_max_concurrent() -> usize {
    4
}

fn bad(message: impl std::fmt::Display) -> String {
    format!("Invalid tiering configuration: {message}")
}

impl TieringConfig {
    /// Checks every range and the shape of the ladder. A configuration that
    /// fails here stops the boot.
    pub fn validate(&self) -> Result<(), String> {
        if self.cost_unit.is_empty()
            || self.cost_unit.len() > 8
            || !self.cost_unit.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err(bad("cost_unit must be 1 to 8 alphanumeric characters"));
        }
        if self.tiers.is_empty() || self.tiers.len() > 8 {
            return Err(bad("tiers must declare between 1 and 8 tiers"));
        }
        let ranges = [
            ("max_digests", self.max_digests as u64, 1, 1_000),
            ("max_rows", self.max_rows as u64, 1, 100_000),
            (
                "max_response_bytes",
                self.max_response_bytes as u64,
                1_024,
                4 * 1024 * 1024,
            ),
            ("timeout_secs", self.timeout_secs, 1, 120),
            ("max_concurrent", self.max_concurrent as u64, 1, 32),
        ];
        for (name, value, low, high) in ranges {
            if !(low..=high).contains(&value) {
                return Err(bad(format_args!(
                    "{name} must be between {low} and {high}, and {value} was given"
                )));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for (index, tier) in self.tiers.iter().enumerate() {
            if tier.name.is_empty()
                || tier.name.len() > 32
                || !tier
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
            {
                return Err(bad(format_args!(
                    "tier name '{}' must match [a-z0-9_-]{{1,32}}",
                    tier.name
                )));
            }
            if !seen.insert(tier.name.as_str()) {
                return Err(bad(format_args!("tier '{}' is declared twice", tier.name)));
            }
            if tier.monthly_cost_micro_per_gib > 1_000_000_000_000 {
                return Err(bad(format_args!(
                    "tier '{}': monthly_cost_micro_per_gib is above 1000000000000",
                    tier.name
                )));
            }
            if index == 0 {
                if tier.min_age_secs != 0
                    || tier.min_size_bytes != 0
                    || tier.max_references.is_some()
                {
                    return Err(bad(format_args!(
                        "the hottest tier '{}' is the baseline and takes no conditions",
                        tier.name
                    )));
                }
            } else {
                let hotter = &self.tiers[index - 1];
                if tier.monthly_cost_micro_per_gib > hotter.monthly_cost_micro_per_gib {
                    return Err(bad(format_args!(
                        "tier '{}' costs more than the hotter tier '{}': tiers go from hottest to coldest, cheapest last",
                        tier.name, hotter.name
                    )));
                }
                if tier.min_age_secs < hotter.min_age_secs {
                    return Err(bad(format_args!(
                        "tier '{}' accepts younger blobs than the hotter tier '{}': min_age_secs must not decrease",
                        tier.name, hotter.name
                    )));
                }
            }
            if tier.min_age_secs > 10 * 365 * 24 * 3600 {
                return Err(bad(format_args!(
                    "tier '{}': min_age_secs is above ten years",
                    tier.name
                )));
            }
        }
        Ok(())
    }
}

/// What is known of one blob. `age_secs` is `None` when the store keeps no date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Facts {
    /// The bytes of the blob.
    pub size_bytes: u64,
    /// Seconds since it was last written or reused, if the store says.
    pub age_secs: Option<u64>,
    /// How many roots of the repository name this digest.
    pub references: u32,
}

/// One cited fact: what was measured and the rule it was compared with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Citation {
    /// The fact: `size_bytes`, `age_secs`, `references` or `cost`.
    pub fact: &'static str,
    /// The value used, when it has one.
    pub value: Option<u64>,
    /// The rule it satisfied, or the one that kept a colder tier away.
    pub rule: String,
}

/// The recommendation for one blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Advice {
    /// The coldest eligible tier.
    pub recommended_tier: String,
    /// The facts the decision rests on.
    pub facts: Facts,
    /// Every fact used, with the rule it met.
    pub reasons: Vec<Citation>,
    /// Why the next colder tier was not recommended, if there is one.
    pub colder_tier_blocked_by: Option<Citation>,
    /// Monthly cost of the blob in the recommended tier (millionths of the unit).
    pub monthly_cost_micro: u128,
    /// Monthly cost in the hottest tier, for comparison.
    pub hottest_monthly_cost_micro: u128,
    /// The facts the platform does not have and the advice could not use.
    pub missing_facts: Vec<&'static str>,
}

fn monthly_cost(size_bytes: u64, micro_per_gib: u64) -> u128 {
    // Rounded up: an estimate never understates a cost to zero for a real blob.
    (u128::from(size_bytes) * u128::from(micro_per_gib)).div_ceil(BYTES_PER_GIB)
}

/// The first condition of `tier` that `facts` do not meet, if any.
fn blocker(tier: &TierConfig, facts: &Facts) -> Option<Citation> {
    match facts.age_secs {
        None if tier.min_age_secs > 0 => {
            return Some(Citation {
                fact: "age_secs",
                value: None,
                rule: format!(
                    "unknown age cannot satisfy min_age_secs {} of tier '{}'",
                    tier.min_age_secs, tier.name
                ),
            });
        }
        Some(age) if age < tier.min_age_secs => {
            return Some(Citation {
                fact: "age_secs",
                value: Some(age),
                rule: format!(
                    "< min_age_secs {} of tier '{}'",
                    tier.min_age_secs, tier.name
                ),
            });
        }
        _ => {}
    }
    if facts.size_bytes < tier.min_size_bytes {
        return Some(Citation {
            fact: "size_bytes",
            value: Some(facts.size_bytes),
            rule: format!(
                "< min_size_bytes {} of tier '{}'",
                tier.min_size_bytes, tier.name
            ),
        });
    }
    if let Some(max) = tier.max_references
        && facts.references > max
    {
        return Some(Citation {
            fact: "references",
            value: Some(u64::from(facts.references)),
            rule: format!("> max_references {max} of tier '{}'", tier.name),
        });
    }
    None
}

/// The pure decision. `tiers` must have passed [`TieringConfig::validate`].
///
/// Returns `None` only for an empty ladder (a configuration that cannot exist
/// once validated), so the caller fails closed instead of inventing a tier.
pub fn advise(tiers: &[TierConfig], facts: Facts) -> Option<Advice> {
    let hottest = tiers.first()?;
    let chosen = tiers
        .iter()
        .rposition(|tier| blocker(tier, &facts).is_none())?;
    let tier = &tiers[chosen];
    let mut reasons = Vec::new();
    if tier.min_age_secs > 0 {
        reasons.push(Citation {
            fact: "age_secs",
            value: facts.age_secs,
            rule: format!(
                ">= min_age_secs {} of tier '{}'",
                tier.min_age_secs, tier.name
            ),
        });
    }
    if tier.min_size_bytes > 0 {
        reasons.push(Citation {
            fact: "size_bytes",
            value: Some(facts.size_bytes),
            rule: format!(
                ">= min_size_bytes {} of tier '{}'",
                tier.min_size_bytes, tier.name
            ),
        });
    }
    if let Some(max) = tier.max_references {
        reasons.push(Citation {
            fact: "references",
            value: Some(u64::from(facts.references)),
            rule: format!("<= max_references {max} of tier '{}'", tier.name),
        });
    }
    let cost = monthly_cost(facts.size_bytes, tier.monthly_cost_micro_per_gib);
    reasons.push(Citation {
        fact: "cost",
        value: u64::try_from(cost).ok(),
        rule: format!(
            "{} micro per GiB-month of tier '{}'",
            tier.monthly_cost_micro_per_gib, tier.name
        ),
    });
    let mut missing_facts = MISSING_FACTS.to_vec();
    if facts.age_secs.is_none() {
        missing_facts.push("age_secs");
    }
    Some(Advice {
        recommended_tier: tier.name.clone(),
        facts,
        reasons,
        colder_tier_blocked_by: tiers.get(chosen + 1).and_then(|next| blocker(next, &facts)),
        monthly_cost_micro: cost,
        hottest_monthly_cost_micro: monthly_cost(
            facts.size_bytes,
            hottest.monthly_cost_micro_per_gib,
        ),
        missing_facts,
    })
}

/// The validated advisor: the ladder, the bounds and the gate on concurrency.
/// It holds no connection and writes nothing.
#[derive(Debug)]
pub struct TieringAdvisor {
    config: TieringConfig,
    permits: std::sync::Arc<tokio::sync::Semaphore>,
}

impl TieringAdvisor {
    /// Builds the advisor from a configuration, validating it again so no
    /// caller can hold an unchecked one.
    pub fn new(config: TieringConfig) -> Result<Self, String> {
        config.validate()?;
        let permits = std::sync::Arc::new(tokio::sync::Semaphore::new(config.max_concurrent));
        Ok(Self { config, permits })
    }

    /// The validated configuration.
    pub fn config(&self) -> &TieringConfig {
        &self.config
    }

    /// A place among the requests served at once, or `None` when all are taken.
    pub fn try_reserve(&self) -> Option<tokio::sync::OwnedSemaphorePermit> {
        self.permits.clone().try_acquire_owned().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    fn tier(name: &str, cost: u64, age: u64, size: u64, refs: Option<u32>) -> TierConfig {
        TierConfig {
            name: name.to_string(),
            monthly_cost_micro_per_gib: cost,
            min_age_secs: age,
            min_size_bytes: size,
            max_references: refs,
        }
    }

    fn ladder() -> Vec<TierConfig> {
        vec![
            tier("hot", 23_000_000, 0, 0, None),
            tier("warm", 12_500_000, 30 * DAY, 1024, None),
            tier("cold", 4_000_000, 180 * DAY, 1024, Some(1)),
        ]
    }

    fn config(tiers: Vec<TierConfig>) -> TieringConfig {
        TieringConfig {
            cost_unit: default_cost_unit(),
            tiers,
            max_digests: 200,
            max_rows: 10_000,
            max_response_bytes: 256 * 1024,
            timeout_secs: 20,
            max_concurrent: 4,
        }
    }

    fn facts(size: u64, age: Option<u64>, refs: u32) -> Facts {
        Facts {
            size_bytes: size,
            age_secs: age,
            references: refs,
        }
    }

    #[test]
    fn a_young_blob_stays_hot_and_cites_the_cost() {
        let advice = advise(&ladder(), facts(1 << 20, Some(DAY), 1)).unwrap();
        assert_eq!(advice.recommended_tier, "hot");
        assert_eq!(
            advice.colder_tier_blocked_by.unwrap().fact,
            "age_secs",
            "the warm tier is refused because of the age"
        );
        assert!(advice.reasons.iter().any(|c| c.fact == "cost"));
    }

    #[test]
    fn an_old_unshared_blob_goes_to_the_coldest_tier_citing_its_facts() {
        let advice = advise(&ladder(), facts(1 << 30, Some(200 * DAY), 1)).unwrap();
        assert_eq!(advice.recommended_tier, "cold");
        let facts_cited: Vec<_> = advice.reasons.iter().map(|c| c.fact).collect();
        assert_eq!(
            facts_cited,
            ["age_secs", "size_bytes", "references", "cost"]
        );
        assert_eq!(advice.monthly_cost_micro, 4_000_000);
        assert_eq!(advice.hottest_monthly_cost_micro, 23_000_000);
    }

    #[test]
    fn a_shared_old_blob_stops_at_the_tier_that_tolerates_sharing() {
        let advice = advise(&ladder(), facts(1 << 20, Some(200 * DAY), 3)).unwrap();
        assert_eq!(advice.recommended_tier, "warm");
        let blocked = advice.colder_tier_blocked_by.unwrap();
        assert_eq!((blocked.fact, blocked.value), ("references", Some(3)));
    }

    #[test]
    fn a_tiny_blob_is_not_worth_moving() {
        let advice = advise(&ladder(), facts(10, Some(400 * DAY), 1)).unwrap();
        assert_eq!(advice.recommended_tier, "hot");
        assert_eq!(advice.colder_tier_blocked_by.unwrap().fact, "size_bytes");
    }

    #[test]
    fn an_unknown_age_is_never_taken_for_old_and_is_declared_missing() {
        let advice = advise(&ladder(), facts(1 << 30, None, 1)).unwrap();
        assert_eq!(advice.recommended_tier, "hot");
        assert!(advice.missing_facts.contains(&"age_secs"));
    }

    #[test]
    fn the_access_facts_are_always_declared_missing() {
        let advice = advise(&ladder(), facts(1 << 30, Some(DAY), 1)).unwrap();
        assert!(advice.missing_facts.contains(&"access_frequency"));
        assert!(advice.missing_facts.contains(&"last_access"));
    }

    #[test]
    fn the_cost_is_rounded_up_and_does_not_overflow() {
        assert_eq!(monthly_cost(1, 1), 1);
        assert_eq!(monthly_cost(0, 1_000_000_000_000), 0);
        assert!(monthly_cost(u64::MAX, 1_000_000_000_000) > 0);
    }

    #[test]
    fn an_empty_ladder_gives_no_advice() {
        assert!(advise(&[], facts(1, Some(1), 1)).is_none());
    }

    #[test]
    fn the_validation_refuses_what_would_make_a_wrong_ladder() {
        let refused = |tiers: Vec<TierConfig>, needle: &str| {
            let error = config(tiers).validate().unwrap_err();
            assert!(error.contains(needle), "{error}");
        };
        refused(vec![], "between 1 and 8");
        refused(vec![tier("Hot", 1, 0, 0, None)], "must match");
        refused(
            vec![tier("a", 1, 0, 0, None), tier("a", 1, 1, 0, None)],
            "declared twice",
        );
        refused(vec![tier("a", 1, 5, 0, None)], "baseline");
        refused(
            vec![tier("a", 1, 0, 0, None), tier("b", 2, 1, 0, None)],
            "costs more",
        );
        refused(
            vec![
                tier("a", 9, 0, 0, None),
                tier("b", 5, 10, 0, None),
                tier("c", 4, 5, 0, None),
            ],
            "must not decrease",
        );
        refused(vec![tier("a", 2_000_000_000_000, 0, 0, None)], "above");
        let mut c = config(ladder());
        c.max_digests = 0;
        assert!(c.validate().unwrap_err().contains("max_digests"));
        c = config(ladder());
        c.timeout_secs = 121;
        assert!(c.validate().unwrap_err().contains("timeout_secs"));
        c = config(ladder());
        c.max_response_bytes = 10;
        assert!(c.validate().unwrap_err().contains("max_response_bytes"));
        assert!(config(ladder()).validate().is_ok());
    }

    #[test]
    fn an_unknown_field_is_refused_when_the_section_is_read() {
        let text = r#"{"tiers":[{"name":"hot","monthly_cost_micro_per_gib":1}],"surprise":1}"#;
        assert!(serde_json::from_str::<TieringConfig>(text).is_err());
        let text = r#"{"tiers":[{"name":"hot","monthly_cost_micro_per_gib":1,"typo":1}]}"#;
        assert!(serde_json::from_str::<TieringConfig>(text).is_err());
    }

    /// A deterministic sweep of the fact space (no external generator): sizes,
    /// ages (known and unknown) and reference counts around every threshold.
    fn space() -> Vec<Facts> {
        let sizes = [0, 1, 1023, 1024, 1025, 1 << 20, 1 << 30, 1 << 40, u64::MAX];
        let ages = [
            None,
            Some(0),
            Some(30 * DAY - 1),
            Some(30 * DAY),
            Some(180 * DAY - 1),
            Some(180 * DAY),
            Some(u64::MAX),
        ];
        let refs = [0, 1, 2, 5, u32::MAX];
        let mut all = Vec::new();
        for size in sizes {
            for age in ages {
                for references in refs {
                    all.push(facts(size, age, references));
                }
            }
        }
        all
    }

    fn rank(tiers: &[TierConfig], f: Facts) -> usize {
        let advice = advise(tiers, f).unwrap();
        tiers
            .iter()
            .position(|t| t.name == advice.recommended_tier)
            .unwrap()
    }

    #[test]
    fn property_the_advice_is_the_coldest_eligible_tier() {
        let tiers = ladder();
        for f in space() {
            let index = rank(&tiers, f);
            assert!(blocker(&tiers[index], &f).is_none(), "{f:?}");
            for colder in &tiers[index + 1..] {
                assert!(blocker(colder, &f).is_some(), "{f:?}");
            }
        }
    }

    #[test]
    fn property_ageing_never_warms_a_blob() {
        let tiers = ladder();
        for f in space() {
            let Some(age) = f.age_secs else { continue };
            let older = Facts {
                age_secs: Some(age.saturating_add(DAY)),
                ..f
            };
            assert!(rank(&tiers, older) >= rank(&tiers, f), "{f:?}");
        }
    }

    #[test]
    fn property_the_recommendation_never_costs_more_than_the_hottest() {
        for f in space() {
            let advice = advise(&ladder(), f).unwrap();
            assert!(advice.monthly_cost_micro <= advice.hottest_monthly_cost_micro);
        }
    }

    #[test]
    fn property_a_validated_ladder_always_advises_and_declares_the_missing() {
        assert!(config(ladder()).validate().is_ok());
        for f in space() {
            let advice = advise(&ladder(), f).unwrap();
            assert!(advice.missing_facts.contains(&"access_frequency"));
            assert_eq!(
                advice.missing_facts.contains(&"age_secs"),
                f.age_secs.is_none()
            );
        }
    }
}
