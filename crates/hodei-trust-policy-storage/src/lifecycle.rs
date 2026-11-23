//! The lifecycle over SurrealDB: revisions, heads and events in one place.
//!
//! This is the persistence of [`hodei_trust_policy::lifecycle`], and it
//! carries the property the domain promises: an activation is a
//! **compare-and-set inside a single transaction**. The revision the caller
//! expected as the head is re-checked by the database itself, after the
//! transaction has begun and before anything is written — the guard is a
//! `THROW` in the same statement batch as the writes, so a loser of the
//! race aborts with nothing written and the head it finds names what it
//! lost to.
//!
//! This is the pattern the catalogue uses for its conditional writes (the
//! precondition guard inside `BEGIN TRANSACTION` / `COMMIT`), applied here
//! to policy heads. A read-then-write outside the transaction would be a
//! silent no-op under concurrency, which is the failure HSP-176/178/183/184
//! fixed elsewhere; the guard inside the transaction is what makes "the
//! effective policy is the published revision" true even when two
//! approvers click at the same moment.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_trust_policy::lifecycle::{
    activation_event, invalidation_event, saved_event, validate_policy, Actors, LifecycleError,
    LifecycleEvent, PolicyRevision, SeparationPolicy, MAX_REVISIONS, ROLLBACK_PREFIX,
};
use hodei_trust_policy::policy::TrustPolicy;
use hodei_trust_policy::ports::{DecisionStoreError, PolicyLoader};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use surrealdb::sql::Thing;
use surrealdb::{Connection, Surreal};

/// The table the revisions live in, keyed by `policy:sequence`.
pub const REVISION_TABLE: &str = "policy_revision";

/// The table the heads live in, keyed by `policy`.
pub const HEAD_TABLE: &str = "policy_head";

/// The table the lifecycle events live in.
pub const EVENT_TABLE: &str = "policy_lifecycle_event";

/// The message the in-transaction guard throws.
///
/// The store matches on it to tell a lost race from a broken database: the
/// first is an answer, the second is not.
pub const GUARD_MESSAGE: &str = "the lifecycle guard does not hold";

/// The row of a revision. The whole revision travels as one document: a
/// policy that can be read in halves is a policy that can be activated by
/// halves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionRow {
    /// The policy set.
    pub policy_id: String,
    /// The position in the history.
    pub sequence: i64,
    /// The digest the content has to hash to.
    pub digest: String,
    /// Who saved it.
    pub saved_by: String,
    /// When it was saved.
    pub saved_at: DateTime<Utc>,
    /// Why it exists.
    pub reason: String,
    /// The revision as a whole.
    pub revision: serde_json::Value,
}

/// The row of a head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadRow {
    /// The policy set.
    pub policy_id: String,
    /// The revision that decides.
    pub sequence: i64,
    /// The digest of that revision.
    pub digest: String,
    /// Who switched it on.
    pub activated_by: String,
    /// Who saved the revision that is active.
    pub editor: String,
    /// Why it was activated.
    pub reason: String,
    /// When.
    pub activated_at: DateTime<Utc>,
}

/// The row of an event. The event travels whole; the queryable fields are
/// duplicated so the history can be read without parsing every document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRow {
    /// The policy set.
    pub policy_id: String,
    /// The kind of event.
    pub kind: String,
    /// When it happened.
    pub at: DateTime<Utc>,
    /// The event as a whole.
    pub event: serde_json::Value,
}

/// The outcome of one transaction: nothing when it committed, the message
/// when it failed. The message is what the lifecycle reads — it tells a
/// lost race from a broken store — and SurrealDB's own error is a wide type
/// the rest of the code never needs.
fn the_outcome(response: Result<surrealdb::Response, surrealdb::Error>) -> Result<(), String> {
    match response {
        Ok(response) => response
            .check()
            .map(|_| ())
            .map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    }
}

fn revision_id(policy_id: &str, sequence: u64) -> Thing {
    Thing::from((
        REVISION_TABLE.to_string(),
        format!("{policy_id}#{sequence}"),
    ))
}

fn head_id(policy_id: &str) -> Thing {
    Thing::from((HEAD_TABLE.to_string(), policy_id.to_string()))
}

fn row_from(revision: &PolicyRevision) -> RevisionRow {
    RevisionRow {
        policy_id: revision.policy_id.clone(),
        sequence: revision.sequence as i64,
        digest: revision.digest.clone(),
        saved_by: revision.saved_by.clone(),
        saved_at: revision.saved_at,
        reason: revision.reason.clone(),
        revision: serde_json::to_value(revision).unwrap_or(serde_json::Value::Null),
    }
}

fn revision_from(row: &RevisionRow) -> Option<PolicyRevision> {
    serde_json::from_value(row.revision.clone()).ok()
}

fn event_row(event: &LifecycleEvent) -> EventRow {
    EventRow {
        policy_id: event.policy_id.clone(),
        kind: event.kind.clone(),
        at: event.at,
        event: serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
    }
}

/// The lifecycle store over SurrealDB.
#[derive(Clone)]
pub struct SurrealLifecycleStore<C: Connection> {
    db: Arc<Surreal<C>>,
    /// How many times a transaction conflict is retried before the answer
    /// is given up. The budget is generous on purpose: a save reads the
    /// last sequence and a concurrent save that lands first makes every
    /// other writer re-read and re-attempt, so ten simultaneous drafts
    /// need ten successful attempts' worth of churn, and losing a draft to
    /// a busy store would be a worse answer than waiting for it.
    save_retries: usize,
    /// The activation is a single CAS and the loser is classified by
    /// re-reading the head, so it needs fewer attempts.
    activate_retries: usize,
    /// The nonce that makes every event row unique, even when the same
    /// revision is activated twice or the clock stands still.
    nonce: Arc<AtomicU64>,
    /// The separation of editor and approver the deployment enforces. It
    /// is checked here, where the actor and the revision's editor meet,
    /// and it is recorded in every head row and event, so the audit can
    /// read who approved whose work (AC4).
    separation: SeparationPolicy,
}

impl<C: Connection> SurrealLifecycleStore<C> {
    /// A lifecycle store over a database, with the separation the
    /// deployment asks for.
    pub fn with_separation(db: Arc<Surreal<C>>, separation: SeparationPolicy) -> Self {
        Self {
            db,
            save_retries: 64,
            activate_retries: 32,
            nonce: Arc::new(AtomicU64::new(0)),
            separation,
        }
    }

    /// A lifecycle store over a database that does not require the
    /// separation.
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self::with_separation(db, SeparationPolicy::default())
    }

    /// Saves a validated policy as a new immutable revision. The sequence is
    /// re-checked inside the transaction, so two concurrent saves cannot
    /// land on the same revision.
    pub async fn save(
        &self,
        policy: TrustPolicy,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<PolicyRevision, LifecycleError> {
        validate_policy(&policy)?;
        if actor.trim().is_empty() {
            return Err(LifecycleError::InvalidPolicy(
                policy.id.clone(),
                "an anonymous actor cannot save a revision".to_string(),
            ));
        }

        let mut last_error = String::new();
        for _ in 0..self.save_retries {
            let known = self
                .last_saved(&policy.id)
                .await?
                .max(self.head_sequence(&policy.id).await?);
            let sequence = known + 1;
            if sequence > MAX_REVISIONS {
                return Err(LifecycleError::HistoryFull(policy.id.clone()));
            }
            let revision = PolicyRevision::new(policy.clone(), sequence, actor, at, reason);
            let row = row_from(&revision);
            let event = saved_event(&revision, at);

            // La secuencia se vuelve a comprobar **dentro** de la
            // transaccion: si otro escritor publico la misma revision
            // mientras se leia, el CREATE choca, la transaccion aborta y
            // no queda ninguna fila escrita.
            let sql = "BEGIN TRANSACTION;\n\
                       LET $existing = (SELECT VALUE sequence FROM type::table($table) \
                        WHERE id = $revision_id);\n\
                       IF $existing != [] { THROW 'the lifecycle guard does not hold' };\n\
                       CREATE $revision_id CONTENT $revision;\n\
                       CREATE $event_id CONTENT $event;\n\
                       COMMIT;\n";
            let result = the_outcome(
                self.db
                    .query(sql)
                    .bind(("table", REVISION_TABLE.to_string()))
                    .bind((
                        "revision_id",
                        revision_id(&revision.policy_id, revision.sequence),
                    ))
                    .bind(("revision", row))
                    .bind(("event_id", self.stamped_event_id(&event.id, "saved")))
                    .bind(("event", event_row(&event)))
                    .await,
            );

            match result {
                Ok(()) => return Ok(revision),
                Err(message) => {
                    // SurrealDB no propaga el mensaje del `THROW` a traves
                    // del envoltorio de la transaccion abortada, asi que la
                    // clasificacion se hace releyendo, como en el patron del
                    // catalogo: si la revision que intentamos escribir ya
                    // esta ahi, perdimos la carrera por esa secuencia. Una
                    // transaccion fallida siempre es reintentable — se cede
                    // el executor y se vuelve a leer la ultima secuencia —;
                    // un fallo que no lo es se responde como lo que es.
                    last_error = message.clone();
                    let lost = self.revision(&policy.id, sequence).await?.is_some();
                    if !lost
                        && !hodei_evidence_storage::is_conflict(&message)
                        && !message.contains(GUARD_MESSAGE)
                        && !message.contains("failed transaction")
                    {
                        return Err(LifecycleError::Unavailable(message));
                    }
                    tokio::task::yield_now().await;
                }
            }
        }
        Err(LifecycleError::Unavailable(format!(
            "the revision could not be written: {last_error}"
        )))
    }

    /// Activates an already-saved revision with a compare-and-set on the
    /// head.
    ///
    /// `expected_head` is the revision the caller believes decides; `None`
    /// means "nothing has ever been activated". The guard re-checks the
    /// expected head **and the digest of the revision inside the
    /// transaction**, so a concurrent winner makes this call fail with
    /// [`LifecycleError::StaleHead`] and nothing is written.
    pub async fn activate(
        &self,
        policy_id: &str,
        sequence: u64,
        expected_head: Option<u64>,
        approver: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<HeadRow, LifecycleError> {
        if approver.trim().is_empty() {
            return Err(LifecycleError::InvalidPolicy(
                policy_id.to_string(),
                "an anonymous actor cannot activate a revision".to_string(),
            ));
        }
        let revision = self
            .revision(policy_id, sequence)
            .await?
            .ok_or_else(|| LifecycleError::RevisionNotFound(policy_id.to_string(), sequence))?;
        if !revision.digest_holds() {
            return Err(LifecycleError::CorruptRevision(
                policy_id.to_string(),
                sequence,
            ));
        }
        self.separation
            .check_approver(policy_id, &revision.saved_by, approver)?;

        let mut last_error = String::new();
        for _ in 0..self.activate_retries {
            let head = HeadRow {
                policy_id: policy_id.to_string(),
                sequence: revision.sequence as i64,
                digest: revision.digest.clone(),
                activated_by: approver.to_string(),
                editor: revision.saved_by.clone(),
                reason: reason.to_string(),
                activated_at: at,
            };
            let event = activation_event(&revision, expected_head, reason, approver, at);

            // El guardia compara la cabeza esperada con la que hay y el
            // digest del contenido con el de la revision apuntada, dentro
            // de la misma transaccion: el perdedor de la carrera aborta sin
            // escribir nada. Las dos comprobaciones son de igualdad de
            // arrays, que SurrealQL resuelve sin ramas.
            let sql = "BEGIN TRANSACTION;\n\
                       LET $heads = (SELECT VALUE sequence FROM type::table($head_table) \
                        WHERE id = $head_id);\n\
                       IF $heads != $expected_heads { THROW 'the lifecycle guard does not hold' };\n\
                       LET $digests = (SELECT VALUE digest FROM type::table($revision_table) \
                        WHERE id = $revision_id);\n\
                       IF $digests != $expected_digests { THROW 'the lifecycle guard does not hold' };\n\
                       UPSERT $head_id CONTENT $head;\n\
                       CREATE $event_id CONTENT $event;\n\
                       COMMIT;\n";
            let result = the_outcome(
                self.db
                    .query(sql)
                    .bind(("head_table", HEAD_TABLE.to_string()))
                    .bind(("revision_table", REVISION_TABLE.to_string()))
                    .bind(("head_id", head_id(policy_id)))
                    .bind(("revision_id", revision_id(policy_id, revision.sequence)))
                    .bind((
                        "expected_heads",
                        vec![
                            expected_head.unwrap_or(0) as i64;
                            usize::from(expected_head.is_some())
                        ],
                    ))
                    .bind(("expected_digests", vec![revision.digest.clone()]))
                    .bind(("head", head.clone()))
                    .bind(("event_id", self.stamped_event_id(&event.id, "activated")))
                    .bind(("event", event_row(&event)))
                    .await,
            );

            match result {
                Ok(()) => return Ok(head),
                Err(message) => {
                    // El mismo caso que en el guardado: la transaccion
                    // abortada no propaga el motivo, asi que se relee. Si
                    // la cabecera ya no es la esperada, la carrera esta
                    // perdida y eso se responde como lo que es; si sigue
                    // siendo la esperada, el commit choco por otra escritura
                    // concurrente sobre las mismas filas y se reintenta.
                    last_error = message.clone();
                    if message.contains(GUARD_MESSAGE) || message.contains("failed transaction") {
                        let actual = self.head_sequence(policy_id).await?;
                        if Some(actual) != expected_head {
                            return Err(LifecycleError::StaleHead(
                                policy_id.to_string(),
                                expected_head.unwrap_or(0),
                                actual,
                            ));
                        }
                        if hodei_evidence_storage::is_conflict(&message)
                            || message.contains(GUARD_MESSAGE)
                        {
                            tokio::task::yield_now().await;
                            continue;
                        }
                    }
                    if hodei_evidence_storage::is_conflict(&message) {
                        tokio::task::yield_now().await;
                        continue;
                    }
                    return Err(LifecycleError::Unavailable(message));
                }
            }
        }
        Err(LifecycleError::Unavailable(format!(
            "the activation could not be written: {last_error}"
        )))
    }

    /// Rolls an older revision back as a **new** revision, under the same
    /// compare-and-set the activation uses.
    ///
    /// The history is never rewritten: the revision rolled back to keeps
    /// its sequence, and the rollback is a fresh revision whose reason is
    /// marked with [`ROLLBACK_PREFIX`], followed by an `invalidated` event
    /// that names the revision that stopped deciding.
    pub async fn rollback(
        &self,
        policy_id: &str,
        target: u64,
        expected_head: Option<u64>,
        actors: &Actors,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<HeadRow, LifecycleError> {
        let editor = actors.editor.as_str();
        let approver = actors.approver.as_str();
        let head = self
            .head(policy_id)
            .await?
            .ok_or_else(|| LifecycleError::NeverActivated(policy_id.to_string()))?;
        if expected_head != Some(head.sequence as u64) {
            return Err(LifecycleError::StaleHead(
                policy_id.to_string(),
                expected_head.unwrap_or(0),
                head.sequence as u64,
            ));
        }
        if approver.trim().is_empty() {
            return Err(LifecycleError::InvalidPolicy(
                policy_id.to_string(),
                "an anonymous actor cannot roll a revision back".to_string(),
            ));
        }
        self.separation.check_editor(editor)?;
        self.separation
            .check_approver(policy_id, editor, approver)?;
        let source = self
            .revision(policy_id, target)
            .await?
            .ok_or_else(|| LifecycleError::RevisionNotFound(policy_id.to_string(), target))?;

        let sequence = self.last_saved(policy_id).await? + 1;
        if sequence > MAX_REVISIONS {
            return Err(LifecycleError::HistoryFull(policy_id.to_string()));
        }
        let mut policy = source.policy.clone();
        policy.version = format!("{}-rb{sequence}", policy.version);
        let revision = PolicyRevision::new(
            policy,
            sequence,
            editor,
            at,
            format!("{ROLLBACK_PREFIX}{target} {reason}"),
        );
        let row = row_from(&revision);
        let new_head = HeadRow {
            policy_id: policy_id.to_string(),
            sequence: revision.sequence as i64,
            digest: revision.digest.clone(),
            activated_by: approver.to_string(),
            editor: revision.saved_by.clone(),
            reason: reason.to_string(),
            activated_at: at,
        };

        let saved = saved_event(&revision, at);
        let activation =
            activation_event(&revision, Some(head.sequence as u64), reason, approver, at);
        let invalidation = invalidation_event(
            policy_id,
            head.sequence as u64,
            &head.digest,
            &revision,
            approver,
            at,
        );

        let sql = "BEGIN TRANSACTION;\n\
                   LET $heads = (SELECT VALUE sequence FROM type::table($head_table) \
                    WHERE id = $head_id);\n\
                   IF $heads != $expected_heads { THROW 'the lifecycle guard does not hold' };\n\
                   CREATE $revision_id CONTENT $revision;\n\
                   UPSERT $head_id CONTENT $head;\n\
                   CREATE $saved_id CONTENT $saved;\n\
                   CREATE $activation_id CONTENT $activation;\n\
                   CREATE $invalidation_id CONTENT $invalidation;\n\
                   COMMIT;\n";
        let result = the_outcome(
            self.db
                .query(sql)
                .bind(("head_table", HEAD_TABLE.to_string()))
                .bind(("revision_table", REVISION_TABLE.to_string()))
                .bind(("head_id", head_id(policy_id)))
                .bind(("revision_id", revision_id(policy_id, revision.sequence)))
                .bind(("expected_heads", vec![head.sequence]))
                .bind(("revision", row))
                .bind(("head", new_head.clone()))
                .bind((
                    "saved_id",
                    self.stamped_event_id(&saved.id, "rollback-saved"),
                ))
                .bind(("saved", event_row(&saved)))
                .bind((
                    "activation_id",
                    self.stamped_event_id(&activation.id, "rollback-activated"),
                ))
                .bind(("activation", event_row(&activation)))
                .bind((
                    "invalidation_id",
                    self.stamped_event_id(&invalidation.id, "invalidated"),
                ))
                .bind(("invalidation", event_row(&invalidation)))
                .await,
        );

        match result {
            Ok(()) => Ok(new_head),
            Err(message) => {
                // La misma clasificacion por relectura que la activacion:
                // la cabecera ha cambiado o el commit choco con otra
                // escritura sobre las mismas filas.
                let actual = self.head_sequence(policy_id).await?;
                if Some(actual) != expected_head {
                    return Err(LifecycleError::StaleHead(
                        policy_id.to_string(),
                        expected_head.unwrap_or(0),
                        actual,
                    ));
                }
                Err(LifecycleError::Unavailable(message))
            }
        }
    }

    /// The revisions of a policy set, oldest first.
    pub async fn history(&self, policy_id: &str) -> Result<Vec<PolicyRevision>, LifecycleError> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM type::table($table) WHERE policy_id = $policy \
                 ORDER BY sequence ASC",
            )
            .bind(("table", REVISION_TABLE.to_string()))
            .bind(("policy", policy_id.to_string()))
            .await
            .map_err(|error| LifecycleError::Unavailable(error.to_string()))?;
        let rows: Vec<RevisionRow> = response
            .take(0)
            .map_err(|error| LifecycleError::Unavailable(error.to_string()))?;
        Ok(rows.iter().filter_map(revision_from).collect())
    }

    /// One revision by sequence.
    pub async fn revision(
        &self,
        policy_id: &str,
        sequence: u64,
    ) -> Result<Option<PolicyRevision>, LifecycleError> {
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", revision_id(policy_id, sequence)))
            .await
            .map_err(|error| LifecycleError::Unavailable(error.to_string()))?;
        let rows: Vec<RevisionRow> = response
            .take(0)
            .map_err(|error| LifecycleError::Unavailable(error.to_string()))?;
        Ok(rows.first().and_then(revision_from))
    }

    /// The head of a policy set.
    pub async fn head(&self, policy_id: &str) -> Result<Option<HeadRow>, LifecycleError> {
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", head_id(policy_id)))
            .await
            .map_err(|error| LifecycleError::Unavailable(error.to_string()))?;
        let rows: Vec<HeadRow> = response
            .take(0)
            .map_err(|error| LifecycleError::Unavailable(error.to_string()))?;
        Ok(rows.first().cloned())
    }

    /// The sequence of the current head, `0` if there is none.
    pub async fn head_sequence(&self, policy_id: &str) -> Result<u64, LifecycleError> {
        Ok(self
            .head(policy_id)
            .await?
            .map(|head| head.sequence.max(0) as u64)
            .unwrap_or(0))
    }

    /// The last revision ever saved, `0` if there is none.
    async fn last_saved(&self, policy_id: &str) -> Result<u64, LifecycleError> {
        let mut response = self
            .db
            .query(
                "SELECT VALUE sequence FROM type::table($table) WHERE policy_id = $policy \
                 ORDER BY sequence DESC LIMIT 1",
            )
            .bind(("table", REVISION_TABLE.to_string()))
            .bind(("policy", policy_id.to_string()))
            .await
            .map_err(|error| LifecycleError::Unavailable(error.to_string()))?;
        let rows: Vec<i64> = response
            .take(0)
            .map_err(|error| LifecycleError::Unavailable(error.to_string()))?;
        Ok(rows
            .first()
            .map(|sequence| (*sequence).max(0) as u64)
            .unwrap_or(0))
    }

    /// The events of a policy set, oldest first.
    pub async fn events(&self, policy_id: &str) -> Result<Vec<LifecycleEvent>, LifecycleError> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM type::table($table) WHERE policy_id = $policy \
                 ORDER BY at ASC",
            )
            .bind(("table", EVENT_TABLE.to_string()))
            .bind(("policy", policy_id.to_string()))
            .await
            .map_err(|error| LifecycleError::Unavailable(error.to_string()))?;
        let rows: Vec<EventRow> = response
            .take(0)
            .map_err(|error| LifecycleError::Unavailable(error.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|row| serde_json::from_value(row.event.clone()).ok())
            .collect())
    }

    /// A record id for one event, stamped with the process nonce so the
    /// same logical event written twice (a reactivation, a retry) is still
    /// one row and never clobbers another.
    fn stamped_event_id(&self, id: &str, prefix: &str) -> Thing {
        let nonce = self.nonce.fetch_add(1, Ordering::SeqCst);
        Thing::from((EVENT_TABLE.to_string(), format!("{prefix}#{id}#{nonce}")))
    }
}

#[async_trait]
impl<C: Connection> PolicyLoader for SurrealLifecycleStore<C> {
    /// The policy that is in force: the revision the head names, and no
    /// other.
    ///
    /// A set that has never been activated is not a set whose newest
    /// revision decides — it is an error, fail-closed. This is what makes
    /// "the effective policy of the server is the published revision"
    /// checkable end to end (AC4).
    async fn active_policy(&self, policy_id: &str) -> Result<TrustPolicy, DecisionStoreError> {
        let head = self
            .head(policy_id)
            .await
            .map_err(|error| DecisionStoreError::Unavailable(error.to_string()))?
            .ok_or_else(|| {
                DecisionStoreError::Unavailable(format!(
                    "there is no active policy set for {policy_id}"
                ))
            })?;
        let revision = self
            .revision(policy_id, head.sequence as u64)
            .await
            .map_err(|error| DecisionStoreError::Unavailable(error.to_string()))?
            .ok_or_else(|| {
                DecisionStoreError::Unavailable(format!(
                    "the head of {policy_id} names revision {} and it is not there",
                    head.sequence
                ))
            })?;
        if !revision.digest_holds() || revision.digest != head.digest {
            return Err(DecisionStoreError::Unavailable(format!(
                "the revision the head of {policy_id} names does not match its digest"
            )));
        }
        Ok(revision.policy)
    }
}
