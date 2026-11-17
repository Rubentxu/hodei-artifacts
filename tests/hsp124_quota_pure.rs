//! El ledger de cuotas es admision, no contabilidad posterior: la
//! decision de admitir y la reserva del tamano declarado son **el mismo
//! paso atomico**, la cancelacion devuelve lo reservado, y lo que entra
//! por un stream sin tamano declarado se ancla trozo a trozo.

use std::sync::Arc;

use hodei_artifacts_api::quota::{QuotaLedger, QuotaLimits, QuotaRefusal};

fn limits(max_bytes: u64, max_uploads: usize) -> QuotaLimits {
    QuotaLimits {
        max_stored_bytes_per_account: Some(max_bytes),
        max_concurrent_uploads_per_account: Some(max_uploads),
        retry_after_secs: 3,
    }
}

/// AC1: dos uploads concurrentes que juntos pasan la cuota no caben
/// los dos. La unica decision atomicos es check+reserve bajo la misma
/// llave; aqui se comprueba que no hay ventana: pase lo que pase con
/// el orden, el admitido mas el reservado jamas supera el limite.
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

/// AC2: la negativa dice cual de las dos puertas es y cuanto hay que
/// esperar. La de bytes no se quita reintentando; la de concurrencia
/// lleva el hint.
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
    // Otra cuenta no padece la saturacion de esta.
    let other = ledger
        .begin("globex", Some(40))
        .expect("another account is its own budget");
    drop(other);
    drop(first);
    // Y con el ticket cerrado, la misma cuenta vuelve a caber.
    let again = ledger
        .begin("acme", Some(10))
        .expect("the gate opens when the upload ends");
    drop(again);
}

/// AC3: la cancelacion libera. Un ticket abandonado a medias (el
/// cliente cuelga, el servidor aborta) devuelve la reserva y el hueco
/// de concurrencia: exactamente lo que paga el settle, pero en cero.
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

/// Un stream sin Content-Length se ancla trozo a trozo: la cuota se
/// aplica **mientras** entran los bytes, no cuando ya todos estan.
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
    // 30+30+30 = 90: cabe. El siguiente trozo de 20 no.
    let over = ticket
        .admit(total + 20)
        .expect_err("110 does not fit in 100");
    assert_eq!(
        over,
        QuotaRefusal::StorageOver {
            retry_after_secs: 3
        }
    );
    // El rechazo no reservo nada mas: lo anclado sigue siendo 90.
    assert_eq!(ledger.usage("acme").in_flight_bytes, 90);
    ticket.settle(0);
    assert_eq!(
        ledger.usage("acme").in_flight_bytes,
        0,
        "the refused bytes never became stored"
    );
}

/// El settle convierte reserva en almacenado, y lo que el almacen ya
/// tenia (contenido repetido) no cuenta dos veces.
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
    // Publicar contenido que el almacen ya tiene no anade bytes: el
    // settle con cero deja la cuenta exactamente como estaba.
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

/// Un despliegue que no dice nada no limita nada: el limite que no se
/// escribe es un limite que no se aplica, y eso debe ser el default.
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
