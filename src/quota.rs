//! The quota of every account: bytes stored and uploads in flight.
//!
//! A quota is admission, not accounting after the fact: the decision is
//! taken **before** a byte is written, and it is taken once, under one
//! lock, together with the reservation of what it admits. Two uploads
//! that arrive at the same time are decided one after the other against
//! the same state, and what the first reserved the second cannot spend.
//! That is the whole race argument: there is no check-then-write window
//! because the check and the write of the ledger are the same step.
//!
//! The reservation is what makes cancellation honest. A ticket holds the
//! bytes it declared (or the ones it admitted as they streamed) until it
//! either **settles** them into stored bytes or is dropped: dropped
//! without settling is a cancelled upload, and its reservation comes
//! back. An upload that fails mid-stream, one the client hangs up, and
//! one the server refuses all leave the account exactly as it was.
//!
//! The ledger is in memory on purpose: it is a **rate-shaping** device,
//! not a billing book. A restart forgets everything -- in-flight
//! tickets died with the process, and the stored count starts again
//! from zero until new uploads bring it up. That is the same promise
//! the upload session registry makes about what lives in memory, and a
//! deployment that needs money-accurate accounting needs a ledger with
//! different guarantees, which is said here instead of pretended.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// What one account may do, decided by the deployment.
///
/// `None` means the deployment did not say, and not saying is not the
/// same as zero: an unlimited account is the honest default of a service
/// that never advertised quotas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaLimits {
    /// The most bytes this account may have stored.
    pub max_stored_bytes_per_account: Option<u64>,
    /// The most uploads of this account that may be in flight at once.
    ///
    /// An in-flight upload is either streaming or an open chunked
    /// session: exactly the state that costs staging and a live ticket.
    pub max_concurrent_uploads_per_account: Option<usize>,
    /// What `Retry-After` says when the concurrency gate refuses.
    ///
    /// The client is not doing anything wrong; it is doing too much at
    /// once. The only honest answer carries a hint of when to come back.
    pub retry_after_secs: u64,
}

impl Default for QuotaLimits {
    fn default() -> Self {
        Self {
            max_stored_bytes_per_account: None,
            max_concurrent_uploads_per_account: None,
            retry_after_secs: 1,
        }
    }
}

/// Why the ledger refused, in the two languages the client needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuotaRefusal {
    /// The bytes would take the account past its stored limit: the
    /// client must send less, and sending it again later changes
    /// nothing. `413`.
    StorageOver { retry_after_secs: u64 },
    /// The account already has as many uploads in flight as it may:
    /// the same bytes are fine a moment later. `429` with
    /// `Retry-After`.
    UploadsSaturated { retry_after_secs: u64 },
}

impl std::fmt::Display for QuotaRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StorageOver { .. } => {
                formatter.write_str("the upload would take the account past its stored limit")
            }
            Self::UploadsSaturated { .. } => {
                formatter.write_str("the account already has as many uploads in flight as it may")
            }
        }
    }
}

impl std::error::Error for QuotaRefusal {}

/// What one account has, said to its owner and to nobody else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaUsage {
    pub stored_bytes: u64,
    pub in_flight_bytes: u64,
    pub active_uploads: usize,
    pub limits: QuotaLimits,
}

#[derive(Debug, Clone)]
struct AccountState {
    stored: u64,
    in_flight: u64,
    active: usize,
}

#[derive(Debug)]
struct Ledger {
    limits: QuotaLimits,
    accounts: HashMap<String, AccountState>,
}

/// The shared ledger. Cheap to clone the `Arc` of, and one lock guards
/// every decision: the atomicity of the quota is the atomicity of that
/// lock.
#[derive(Debug, Clone)]
pub struct QuotaLedger(Arc<Mutex<Ledger>>);

impl QuotaLedger {
    pub fn new(limits: QuotaLimits) -> Self {
        Self(Arc::new(Mutex::new(Ledger {
            limits,
            accounts: HashMap::new(),
        })))
    }

    /// Admit an upload or refuse it, reserving what it declared.
    ///
    /// `declared` is the `Content-Length` when the client said how much
    /// it sends; `None` is a chunked body whose size nobody promised,
    /// which is admitted against the concurrency gate alone and pinned
    /// later, chunk by chunk, by [`QuotaTicket::admit`].
    pub fn begin(&self, account: &str, declared: Option<u64>) -> Result<QuotaTicket, QuotaRefusal> {
        let mut ledger = self.lock();
        let (max_bytes, max_uploads, retry_after_secs) = (
            ledger.limits.max_stored_bytes_per_account,
            ledger.limits.max_concurrent_uploads_per_account,
            ledger.limits.retry_after_secs,
        );
        if let Some(max) = max_uploads {
            let active = ledger
                .accounts
                .get(account)
                .is_some_and(|a| a.active >= max);
            if active {
                return Err(QuotaRefusal::UploadsSaturated { retry_after_secs });
            }
        }
        let state = ledger
            .accounts
            .entry(account.to_string())
            .or_insert(AccountState {
                stored: 0,
                in_flight: 0,
                active: 0,
            });
        if let Some(size) = declared
            && let Some(max) = max_bytes
            && state.stored + state.in_flight + size > max
        {
            return Err(QuotaRefusal::StorageOver { retry_after_secs });
        }
        state.active += 1;
        state.in_flight += declared.unwrap_or(0);
        Ok(QuotaTicket {
            ledger: QuotaLedger(Arc::clone(&self.0)),
            account: account.to_string(),
            reserved: declared.unwrap_or(0),
            settled: false,
        })
    }

    /// What one account has. Never a view of another one.
    pub fn usage(&self, account: &str) -> QuotaUsage {
        let ledger = self.lock();
        let state = ledger
            .accounts
            .get(account)
            .cloned()
            .unwrap_or(AccountState {
                stored: 0,
                in_flight: 0,
                active: 0,
            });
        QuotaUsage {
            stored_bytes: state.stored,
            in_flight_bytes: state.in_flight,
            active_uploads: state.active,
            limits: ledger.limits.clone(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Ledger> {
        self.0.lock().expect("the quota ledger never panics")
    }
}

/// One admitted upload, from admission to its last byte.
///
/// Dropping it without settling returns the reservation: cancellation,
/// interruption and refusal are the same event to the ledger.
#[derive(Debug)]
pub struct QuotaTicket {
    ledger: QuotaLedger,
    account: String,
    reserved: u64,
    settled: bool,
}

impl QuotaTicket {
    /// Pin a chunk that streamed in as it arrives.
    ///
    /// Returns the bytes **not** yet pinned by a previous `admit`; the
    /// caller hands it the running total. A refusal is the end of this
    /// upload: the reservation it held goes back in the SAME step that
    /// refuses (under the same lock), so a concurrent upload that is
    /// decided a moment later never counts the bytes of an upload that
    /// has already been refused -- the handler still has to abort its
    /// staging write, and that window must not starve the others.
    pub fn admit(&mut self, total_so_far: u64) -> Result<(), QuotaRefusal> {
        let more = total_so_far.saturating_sub(self.reserved);
        if more == 0 {
            return Ok(());
        }
        let mut ledger = self.ledger.lock();
        let (max_bytes, retry_after_secs) = (
            ledger.limits.max_stored_bytes_per_account,
            ledger.limits.retry_after_secs,
        );
        let over = max_bytes.is_some_and(|max| {
            ledger
                .accounts
                .get(&self.account)
                .is_some_and(|state| state.stored + state.in_flight + more > max)
        });
        if over {
            if !self.settled {
                self.settled = true;
                if let Some(state) = ledger.accounts.get_mut(&self.account) {
                    state.in_flight = state.in_flight.saturating_sub(self.reserved);
                    state.active = state.active.saturating_sub(1);
                }
            }
            return Err(QuotaRefusal::StorageOver { retry_after_secs });
        }
        if let Some(state) = ledger.accounts.get_mut(&self.account) {
            state.in_flight += more;
        }
        self.reserved = total_so_far;
        Ok(())
    }

    /// Close the upload against the ledger: the reservation becomes
    /// stored bytes.
    ///
    /// `stored_new_bytes` is what the store gained because of this
    /// upload: content the account already had adds nothing.
    pub fn settle(&mut self, stored_new_bytes: u64) {
        if self.settled {
            return;
        }
        self.settled = true;
        let mut ledger = self.ledger.lock();
        if let Some(state) = ledger.accounts.get_mut(&self.account) {
            state.in_flight = state.in_flight.saturating_sub(self.reserved);
            state.stored = state.stored.saturating_add(stored_new_bytes);
            state.active = state.active.saturating_sub(1);
        }
    }
}

impl Drop for QuotaTicket {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let mut ledger = self.ledger.lock();
        if let Some(state) = ledger.accounts.get_mut(&self.account) {
            state.in_flight = state.in_flight.saturating_sub(self.reserved);
            state.active = state.active.saturating_sub(1);
        }
    }
}
