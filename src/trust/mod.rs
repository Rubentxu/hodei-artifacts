//! Admission to the trusted channel: which digest may be served.
//!
//! Cedar decides **who** can read a repository; this decides **which
//! software** is served to them. They are two separate obligations and both are
//! required: a Cedar permit does not serve a digest without a grant, and a
//! grant does not serve someone Cedar does not let read.
//!
//! Admission is a single point, used by all read paths
//! (raw and OCI). Having each path carry its own check is how you
//! end up with a path that gets forgotten: here there is no other way to serve
//! a digest from a repository with a trusted channel.
//!
//! # Rules
//!
//! - The channel belongs to the repository ([`ServeChannel`]); `open` is the
//!   quarantine one and serves what is published.
//! - On the `trusted` channel only a **digest** with an active grant is
//!   served. The name does not count: a tag that moves points to another
//!   digest, and that one inherits nothing.
//! - What is not admitted answers as **nonexistent**, not as
//!   forbidden: a 403 would say the content exists.
//! - There is no negative cache: each `NoGranted`, each failure and each
//!   missing ledger is asked again on the next request.
//!   Active grants are cached --with expiration and
//!   invalidation on revocation-- in [`cache::CachedTrustLedger`], which
//!   wraps the ledger at service startup.
//! - A ledger that fails, or that is absent, is **not** a permit: the trusted
//!   channel without a ledger answers "unavailable".

pub mod cache;
pub mod hrn;
pub mod promotion;
pub mod reaction;

use std::collections::HashSet;

use chrono::Utc;
use hodei_catalog::{RepositoryDescriptor, ServeChannel};
use hodei_evidence::ContentDigest;

use crate::app_state::AppState;

/// Why something from the trusted channel is not served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// The digest has no active grant: for whoever reads, it does not exist.
    Hidden,
    /// It could not be known: the ledger fails or is absent. It is never served.
    Unavailable,
}

/// What the ledger said, uninterpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    /// There is an active grant.
    Granted,
    /// There is none.
    NotGranted,
    /// The ledger failed.
    Failed,
    /// The instance has no ledger.
    NoLedger,
}

/// The verdict: a pure function of channel and ledger response.
///
/// Everything that is not an explicit `Granted` on the trusted channel is a
/// denial.
pub fn verdict(channel: ServeChannel, lookup: Lookup) -> Result<(), Admission> {
    match (channel, lookup) {
        (ServeChannel::Open, _) => Ok(()),
        (ServeChannel::Trusted, Lookup::Granted) => Ok(()),
        (ServeChannel::Trusted, Lookup::NotGranted) => Err(Admission::Hidden),
        (ServeChannel::Trusted, Lookup::Failed | Lookup::NoLedger) => Err(Admission::Unavailable),
    }
}

/// Whether that digest of the repository may be served.
pub async fn admit(
    state: &AppState,
    account: &str,
    repository: &RepositoryDescriptor,
    digest: &ContentDigest,
) -> Result<(), Admission> {
    let channel = ServeChannel::of(&repository.config);
    if channel == ServeChannel::Open {
        return Ok(());
    }
    let lookup = match state.trust_ledger.as_ref() {
        None => Lookup::NoLedger,
        Some(ledger) => {
            let now = Utc::now();
            match ledger
                .active_grant(account, &repository.repo_key, digest, now)
                .await
            {
                Ok(Some(grant)) if grant.is_active_at(now) => Lookup::Granted,
                Ok(_) => Lookup::NotGranted,
                Err(_) => Lookup::Failed,
            }
        }
    };
    verdict(channel, lookup)
}

/// The servable digests of a repository, to filter listings.
///
/// `Ok(None)` is the open channel: there is nothing to filter. `Ok(Some(..))`
/// is the set of digests with an active grant, in hexadecimal.
pub async fn servable(
    state: &AppState,
    account: &str,
    repository: &RepositoryDescriptor,
) -> Result<Option<HashSet<String>>, Admission> {
    if ServeChannel::of(&repository.config) == ServeChannel::Open {
        return Ok(None);
    }
    let ledger = state.trust_ledger.as_ref().ok_or(Admission::Unavailable)?;
    let now = Utc::now();
    let grants = ledger
        .grants_of(account, &repository.repo_key)
        .await
        .map_err(|_| Admission::Unavailable)?;
    Ok(Some(
        grants
            .into_iter()
            .filter(|grant| grant.is_active_at(now))
            .map(|grant| grant.digest.value)
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_channel_serves_whatever_the_ledger_says() {
        for lookup in [
            Lookup::Granted,
            Lookup::NotGranted,
            Lookup::Failed,
            Lookup::NoLedger,
        ] {
            assert_eq!(verdict(ServeChannel::Open, lookup), Ok(()));
        }
    }

    #[test]
    fn the_trusted_channel_serves_only_an_explicit_grant() {
        assert_eq!(verdict(ServeChannel::Trusted, Lookup::Granted), Ok(()));
        assert_eq!(
            verdict(ServeChannel::Trusted, Lookup::NotGranted),
            Err(Admission::Hidden)
        );
    }

    #[test]
    fn an_unknown_state_is_never_a_permission() {
        for lookup in [Lookup::Failed, Lookup::NoLedger] {
            assert_eq!(
                verdict(ServeChannel::Trusted, lookup),
                Err(Admission::Unavailable),
                "{lookup:?}"
            );
        }
    }
}
