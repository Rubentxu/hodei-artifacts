//! The quota ledger is admission, not after-the-fact accounting: the
//! decision to admit and the reservation of the declared size are **the same
//! atomic step**, cancellation returns what was reserved, and what comes in
//! through a stream without a declared size is anchored chunk by chunk.

use std::sync::Arc;

use hodei_artifacts_api::quota::{QuotaLedger, QuotaLimits, QuotaRefusal};

fn limits(max_bytes: u64, max_uploads: usize) -> QuotaLimits {
    QuotaLimits {
        max_stored_bytes_per_account: Some(max_bytes),
        max_concurrent_uploads_per_account: Some(max_uploads),
        retry_after_secs: 3,
    }
}

/// AC1: two concurrent uploads that together exceed the quota do not both
/// fit. The only atomic decision is check+reserve under the same
/// key; here it is checked that there is no window: whatever happens with
/// the order, admitted plus reserved never exceeds the limit.
#[test]
fn two_concurrent_uploads_that_do_not_both_fit_never_both_reserve() {
    for _ in 0..64 {
        let ledger = Arc::new(QuotaLedger::new(limits(100, 8)));
        let left = Arc::clone(&ledger);
        let right = Arc::clone(&ledger);
        let outcome = std::thread::scope(|scope| {
            let a = scope.spawn(move || left.begin("acme", Some(60)));
            let b = scope.spawn(move || right.begin("acme", Some(60)));
            (a.join().unwrap().is_ok(), b.join().unwrap().is_ok())
        });
        let admitted = usize::from(outcome.0) + usize::from(outcome.1);
        assert_eq!(
            admitted, 1,
            "exactly one of the two must be admitted: {outcome:?}"
        );
        let usage = ledger.usage("acme");
        assert!(
            usage.in_flight_bytes <= 100,
            "the reservation never exceeds the limit: {usage:?}"
        );
    }
}

/// AC2: the refusal says which of the two gates it is and how long to
/// wait. The bytes one is not removed by retrying; the concurrency one
/// carries the hint.
#[test]
fn each_refusal_says_which_gate_it_is() {
    let ledger = QuotaLedger::new(limits(100, 1));
    let over = ledger.begin("acme", Some(150)).expect_err("150 over 100");
    assert_eq!(
        over,
        QuotaRefusal::StorageOver {
            retry_after_secs: 3
        }
    );
    let first = ledger.begin("acme", Some(40)).expect("40 fits");
    let saturated = ledger
        .begin("acme", Some(10))
        .expect_err("one upload in flight");
    assert_eq!(
        saturated,
        QuotaRefusal::UploadsSaturated {
            retry_after_secs: 3
        }
    );
    // Another account does not suffer this one's saturation.
    let other = ledger
        .begin("globex", Some(40))
        .expect("another account is its own budget");
    drop(other);
    drop(first);
    // And with the ticket closed, the same account fits again.
    let again = ledger
        .begin("acme", Some(10))
        .expect("the gate opens when the upload ends");
    drop(again);
}

/// AC3: cancellation releases. A ticket abandoned halfway (the
/// client hangs, the server aborts) returns the reservation and the concurrency
/// slot: exactly what settle pays, but at zero.
#[test]
fn an_abandoned_ticket_returns_everything_it_held() {
    let ledger = QuotaLedger::new(limits(100, 1));
    let mut ticket = ledger.begin("acme", Some(60)).expect("60 fits");
    let mut streamed = 60;
    ticket
        .admit({
            streamed += 20;
            streamed
        })
        .expect("80 still fits");
    assert_eq!(ledger.usage("acme").in_flight_bytes, 80);
    drop(ticket);
    let usage = ledger.usage("acme");
    assert_eq!(usage.in_flight_bytes, 0, "the reservation comes back whole");
    assert_eq!(usage.active_uploads, 0);
    assert_eq!(
        usage.stored_bytes, 0,
        "an upload that never settled stored nothing"
    );
}

/// A stream without Content-Length is anchored chunk by chunk: the quota is
/// applied **while** the bytes come in, not when they are all there.
#[test]
fn an_undeclared_stream_is_pinned_chunk_by_chunk() {
    let ledger = QuotaLedger::new(limits(100, 4));
    let mut ticket = ledger
        .begin("acme", None)
        .expect("no declared size is admitted");
    assert_eq!(
        ledger.usage("acme").in_flight_bytes,
        0,
        "nothing is reserved yet"
    );
    let mut total = 0u64;
    for chunk in [30u64, 30, 30] {
        total += chunk;
        ticket.admit(total).expect("90 still fits");
    }
    // 30+30+30 = 90: it fits. The next chunk of 20 does not.
    let over = ticket
        .admit(total + 20)
        .expect_err("110 does not fit in 100");
    assert_eq!(
        over,
        QuotaRefusal::StorageOver {
            retry_after_secs: 3
        }
    );
    // The rejection reserves nothing more and, in addition, releases what was anchored in the same
    // atomic step: a rejected ticket is no longer a live upload.
    let usage = ledger.usage("acme");
    assert_eq!(
        (usage.in_flight_bytes, usage.active_uploads),
        (0, 0),
        "a refusal releases the reservation of the ticket at once"
    );
    ticket.settle(0);
    assert_eq!(
        ledger.usage("acme").in_flight_bytes,
        0,
        "the refused bytes never became stored"
    );
}

/// HSP-314 P4: two streaming uploads that together exceed the quota. The one that
/// crosses the limit is rejected and its reservation returns in THAT same step: the
/// other, which arrives an instant later -- before the first one's handler
/// has released the ticket --, cannot see the reservation of an upload
/// that is already dead. Before, the rejection left the reservation alive until the `drop` and
/// both uploads were rejected (`[413, 413]`).
#[test]
fn a_refused_ticket_does_not_starve_the_upload_that_follows() {
    let ledger = QuotaLedger::new(limits(1000, 8));
    let mut first = ledger.begin("acme", None).expect("admitted");
    let mut second = ledger.begin("acme", None).expect("admitted");
    first.admit(600).expect("600 fits");
    second.admit(400).expect("1000 fits");
    // The first crosses the limit and is refused. Its handler has not
    // dropped the ticket yet (it is still aborting its staging write).
    first.admit(700).expect_err("1100 does not fit in 1000");
    // The second arrives in that window: the dead upload holds nothing.
    second
        .admit(500)
        .expect("the refused upload released its reservation");
    second.admit(800).expect("800 fits once the first is gone");
    second.settle(800);
    let usage = ledger.usage("acme");
    assert_eq!((usage.stored_bytes, usage.in_flight_bytes), (800, 0));
    assert_eq!(usage.active_uploads, 0);
    drop(first);
    assert_eq!(
        ledger.usage("acme").active_uploads,
        0,
        "dropping the refused ticket must not release twice"
    );
}

/// Settle turns a reservation into stored, and what the store already
/// had (repeated content) does not count twice.
#[test]
fn settling_moves_the_reservation_into_stored_bytes() {
    let ledger = QuotaLedger::new(limits(100, 4));
    let mut ticket = ledger.begin("acme", Some(60)).expect("60 fits");
    ticket.settle(60);
    let usage = ledger.usage("acme");
    assert_eq!(usage.stored_bytes, 60);
    assert_eq!(usage.in_flight_bytes, 0);
    assert_eq!(usage.active_uploads, 0);
    let dup = ledger
        .begin("acme", Some(45))
        .expect_err("only 40 of headroom is left");
    assert!(matches!(
        dup,
        QuotaRefusal::StorageOver {
            retry_after_secs: 3
        }
    ));
    // Publishing content the store already has adds no bytes: the
    // settle with zero leaves the account exactly as it was.
    let mut same = ledger
        .begin("acme", Some(40))
        .expect("40 is exactly the headroom");
    same.settle(0);
    assert_eq!(ledger.usage("acme").stored_bytes, 60);
}

#[test]
fn the_usage_of_one_account_never_names_another() {
    let ledger = QuotaLedger::new(limits(1000, 4));
    let mut alice = ledger.begin("alice", Some(100)).expect("alice uploads");
    alice.settle(100);
    ledger.begin("bob", Some(5)).expect("bob uploads").settle(5);
    let bob = ledger.usage("bob");
    assert_eq!(bob.stored_bytes, 5, "bob sees his own five bytes");
    let fresh = ledger.usage("carol");
    assert_eq!(fresh.stored_bytes, 0);
    assert_eq!(fresh.active_uploads, 0);
}

/// A deployment that says nothing limits nothing: a limit that is not
/// written is a limit that is not applied, and that must be the default.
#[test]
fn an_undeployed_quota_admits_everything() {
    let ledger = QuotaLedger::new(QuotaLimits::default());
    let mut big = ledger
        .begin("acme", Some(u64::MAX / 2))
        .expect("no limit, no refusal");
    big.admit(u64::MAX / 2 + 10)
        .expect("the stream still has no ceiling");
    big.settle(0);
    let tickets: Vec<_> = (0..100)
        .map(|_| {
            ledger
                .begin("acme", None)
                .expect("no concurrency gate either")
        })
        .collect();
    assert_eq!(ledger.usage("acme").active_uploads, tickets.len());
}
