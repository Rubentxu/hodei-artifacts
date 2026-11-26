//! La cache de admision del canal trusted.
//!
//! El libro de concesiones vive en la base, y cada peticion del canal
//! trusted era una lectura suya. Esta cache guarda **solo respuestas
//! afirmativas**: una concesion activa y fresca exime de volver a
//! preguntar; todo lo demas —la ausencia, un fallo del libro— se lee en
//! vivo cada vez, de modo que ningun estado desconocido se convierte en
//! permiso por inercia de la cache.
//!
//! Las reglas que la hacen segura:
//!
//! - **Clave por identidad, no por nombre**: cuenta, repositorio y
//!   digest. Una etiqueta que se mueve a otros bytes consulta una clave
//!   distinta y no hereda la concesion antigua.
//! - **Expiracion**: cada entrada vive un `ttl`; despues la cache vuelve
//!   a preguntar al libro. Es el techo de desactualizacion entre
//!   instancias que no comparten escrituras.
//! - **Invalidacion por cambio relevante**: toda escritura que pasa por
//!   aqui —concesion o revocacion— barre las entradas que toca. Una
//!   revocacion por esta instancia se nota en la peticion siguiente,
//!   como cuando no habia cache.
//! - **Generacion contra lecturas en vuelo**: cada escritura sube un
//!   contador *antes* de tocar el libro, y una lectura cuyo libro
//!   respondio despues de ese salto no inserta su foto: una revocacion
//!   confirmada no puede quedar tapada por la insercion tardia de una
//!   lectura que empezo antes, ni una reconcesion enterrada por la foto
//!   vieja. El limite: una escritura concurrente cuesta como mucho una
//!   lectura extra al libro, nunca un permiso viejo. Una descarga ya
//!   iniciada se sirve con la concesion que admitio su peticion —la
//!   revocacion corta las peticiones nuevas, no las que ya vuelan—,
//!   exactamente como sin cache.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_catalog::{CatalogError, TrustGrant, TrustLedger};
use hodei_evidence::ContentDigest;

/// La duracion por defecto de una entrada: lo que puede tardar en
/// notarse una revocacion hecha por otra instancia.
pub const DEFAULT_TTL: Duration = Duration::from_secs(60);

/// La clave de una decision cacheada: la identidad con la que el libro
/// concede —cuenta, repositorio y digest—, no el nombre por el que el
/// cliente pregunto.
type Key = (String, String, String);

/// Una concesion activa con la hora a la que deja de ser digna de fe.
struct Entry {
    grant: TrustGrant,
    expires_at: Instant,
}

/// El libro envuelto en una cache de solo-positivos.
///
/// Es un `TrustLedger` mas: el estado del servicio no distingue, y las
/// rutas de lectura siguen llamando a `admit` igual que cuando no habia
/// cache. Las escrituras se invalidan a si mismas porque pasan por aqui.
pub struct CachedTrustLedger {
    inner: std::sync::Arc<dyn TrustLedger>,
    ttl: Duration,
    admitted: Mutex<HashMap<Key, Entry>>,
    /// El contador que sube toda escritura antes de tocar el libro. Una
    /// lectura en vuelo que vio una generacion distinta de la actual
    /// cuando el libro le respondio no inserta su foto: puede estar
    /// obsoleta —la revocacion o la reconcesion ya es firme—.
    generation: AtomicU64,
}

impl CachedTrustLedger {
    /// El libro real, con el ttl que debe tener cada entrada.
    pub fn new(inner: std::sync::Arc<dyn TrustLedger>, ttl: Duration) -> Self {
        Self {
            inner,
            ttl,
            admitted: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
        }
    }

    /// El ttl con el que nace la instancia de produccion.
    pub fn with_default_ttl(inner: std::sync::Arc<dyn TrustLedger>) -> Self {
        Self::new(inner, DEFAULT_TTL)
    }

    fn key(account: &str, repository: &str, digest: &ContentDigest) -> Key {
        (
            account.to_string(),
            repository.to_string(),
            digest.to_string(),
        )
    }

    /// La entrada viva de la clave, si existe y no ha caducado.
    fn fresh(&self) -> MutexGuard<'_, HashMap<Key, Entry>> {
        // El mutex puede estar envenenado si una ruta en panico dejo el
        // mapa a medias: la cache se reanuda vacia, que es el lado
        // cerrado —preguntar de nuevo al libro—, no un permiso falso.
        let mut map = self.admitted.lock().unwrap_or_else(|poison| {
            let mut map = poison.into_inner();
            map.clear();
            map
        });
        let now = Instant::now();
        map.retain(|_, entry| entry.expires_at > now);
        map
    }
}

#[async_trait]
impl TrustLedger for CachedTrustLedger {
    async fn grant(&self, grants: &[TrustGrant]) -> Result<(), CatalogError> {
        // Primero la generacion, luego el libro: una lectura que empezo
        // antes de este punto ya no podra insertar su foto vieja cuando
        // el libro le responda.
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.inner.grant(grants).await?;
        // La concesion nueva —o la decision nueva que re-concede algo
        // revocado— invalida su clave: lo que la cache guardaba era la
        // respuesta a una pregunta anterior.
        let mut map = self.fresh();
        for grant in grants {
            map.remove(&Self::key(&grant.account, &grant.repository, &grant.digest));
        }
        Ok(())
    }

    async fn revoke_decision(
        &self,
        account: &str,
        repository: &str,
        decision_id: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let revoked = self
            .inner
            .revoke_decision(account, repository, decision_id, actor, reason, at)
            .await?;
        self.fresh()
            .retain(|(entry_account, entry_repository, _), entry| {
                !(entry_account == account
                    && entry_repository == repository
                    && entry.grant.decision_id == decision_id)
            });
        Ok(revoked)
    }

    async fn revoke_decision_for_account(
        &self,
        account: &str,
        decision_id: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let revoked = self
            .inner
            .revoke_decision_for_account(account, decision_id, actor, reason, at)
            .await?;
        self.fresh().retain(|(entry_account, _, _), entry| {
            !(entry_account == account && entry.grant.decision_id == decision_id)
        });
        Ok(revoked)
    }

    async fn revoke_digest(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let revoked = self
            .inner
            .revoke_digest(account, repository, digest, actor, reason, at)
            .await?;
        self.fresh().remove(&Self::key(account, repository, digest));
        Ok(revoked)
    }

    async fn active_grant(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        at: DateTime<Utc>,
    ) -> Result<Option<TrustGrant>, CatalogError> {
        let key = Self::key(account, repository, digest);
        // La cache acorta el viaje, no la vida de la concesion: una
        // entrada solo sirve si la vigencia del grant que guarda sigue
        // en pie **a la hora preguntada**. El ttl dice cada cuanto se
        // vuelve al libro; `valid_until` dice hasta cuando vale lo que
        // el libro dijo. Son dos relojes distintos, y manda el de la
        // vigencia.
        if let Some(grant) = self
            .fresh()
            .get(&key)
            .filter(|entry| entry.grant.is_active_at(at))
            .map(|entry| entry.grant.clone())
        {
            return Ok(Some(grant));
        }
        let gen_before = self.generation.load(Ordering::SeqCst);
        let grant = self
            .inner
            .active_grant(account, repository, digest, at)
            .await?;
        // Solo la afirmacion **vigente** se cachea: la ausencia, el fallo
        // del libro y lo caducado se vuelven a preguntar siempre, porque
        // una revocacion hecha en otra instancia o un libro que vuelve a
        // vivir deben poder negarse —y conceder— en la peticion siguiente.
        if let Some(active) = grant.as_ref().filter(|grant| grant.is_active_at(at)) {
            // Sujetando el mapa se compara la generacion: si el libro
            // respondio despues de que alguna escritura subiera el
            // contador, su foto puede estar obsoleta y no se inserta.
            // O bien el revoker barre despues de nuestra insercion, o
            // nosotros no llegamos a hacerla; en ningún caso queda un
            // permiso viejo en la cache. Cuesta como mucho una lectura
            // extra al libro, nunca un falso admisible.
            let mut map = self.fresh();
            if gen_before == self.generation.load(Ordering::SeqCst) {
                map.insert(
                    key,
                    Entry {
                        grant: active.clone(),
                        expires_at: Instant::now() + self.ttl,
                    },
                );
            }
        }
        Ok(grant)
    }

    async fn grants_of(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustGrant>, CatalogError> {
        self.inner.grants_of(account, repository).await
    }

    async fn active_digests(
        &self,
        account: &str,
        repository: &str,
        digests: &[ContentDigest],
        at: DateTime<Utc>,
    ) -> Result<Vec<ContentDigest>, CatalogError> {
        // A listing reads the ledger itself: the page is answered by one
        // bounded read of the inner ledger, and a listing does not fill a
        // per-digest cache with the digests it merely names.
        self.inner
            .active_digests(account, repository, digests, at)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// El libro de las pruebas: recuerda sus lecturas y niega lo que le
    /// pongan delante, sin base de datos detras.
    struct FakeLedger {
        grants: tokio::sync::Mutex<Vec<TrustGrant>>,
        reads: AtomicUsize,
    }

    impl FakeLedger {
        fn new(grants: Vec<TrustGrant>) -> Self {
            Self {
                grants: tokio::sync::Mutex::new(grants),
                reads: AtomicUsize::new(0),
            }
        }

        fn the_reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    fn a_grant(digest: &ContentDigest) -> TrustGrant {
        TrustGrant {
            account: "acme".to_string(),
            repository: "trusted-repo".to_string(),
            digest: digest.clone(),
            decision_id: "dec-1".to_string(),
            actor: "promoter".to_string(),
            granted_at: Utc::now(),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: Some("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app".to_string()),
            policy_version: Some("1.0.0".to_string()),
            capsule_hash: Some("sha256:capsule".to_string()),
            risk_version: Some(0),
            valid_until: Some(Utc::now() + chrono::Duration::hours(1)),
        }
    }

    #[async_trait]
    impl TrustLedger for FakeLedger {
        async fn grant(&self, grants: &[TrustGrant]) -> Result<(), CatalogError> {
            self.grants.lock().await.extend_from_slice(grants);
            Ok(())
        }

        async fn revoke_decision(
            &self,
            _account: &str,
            _repository: &str,
            decision_id: &str,
            _actor: &str,
            _reason: &str,
            at: DateTime<Utc>,
        ) -> Result<u64, CatalogError> {
            let mut grants = self.grants.lock().await;
            let mut revoked = 0;
            for grant in grants.iter_mut() {
                if grant.decision_id == decision_id && grant.revoked_at.is_none() {
                    grant.revoked_at = Some(at);
                    revoked += 1;
                }
            }
            Ok(revoked)
        }

        async fn revoke_decision_for_account(
            &self,
            account: &str,
            decision_id: &str,
            _actor: &str,
            _reason: &str,
            at: DateTime<Utc>,
        ) -> Result<u64, CatalogError> {
            let mut grants = self.grants.lock().await;
            let mut revoked = 0;
            for grant in grants.iter_mut() {
                if grant.account == account
                    && grant.decision_id == decision_id
                    && grant.revoked_at.is_none()
                {
                    grant.revoked_at = Some(at);
                    revoked += 1;
                }
            }
            Ok(revoked)
        }

        async fn revoke_digest(
            &self,
            _account: &str,
            _repository: &str,
            digest: &ContentDigest,
            _actor: &str,
            _reason: &str,
            at: DateTime<Utc>,
        ) -> Result<u64, CatalogError> {
            let mut grants = self.grants.lock().await;
            let mut revoked = 0;
            for grant in grants.iter_mut() {
                if &grant.digest == digest && grant.revoked_at.is_none() {
                    grant.revoked_at = Some(at);
                    revoked += 1;
                }
            }
            Ok(revoked)
        }

        async fn active_grant(
            &self,
            _account: &str,
            _repository: &str,
            digest: &ContentDigest,
            at: DateTime<Utc>,
        ) -> Result<Option<TrustGrant>, CatalogError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self
                .grants
                .lock()
                .await
                .iter()
                .find(|grant| &grant.digest == digest && grant.is_active_at(at))
                .cloned())
        }

        async fn grants_of(
            &self,
            _account: &str,
            _repository: &str,
        ) -> Result<Vec<TrustGrant>, CatalogError> {
            Ok(self.grants.lock().await.clone())
        }
    }

    fn the_digest_of(label: &str) -> ContentDigest {
        ContentDigest::of_bytes(label.as_bytes())
    }

    /// La razon de ser de la cache: la segunda peticion del mismo
    /// digest no vuelve al libro.
    #[tokio::test]
    async fn an_admitted_digest_is_asked_to_the_book_once() {
        let digest = the_digest_of("admitted bytes");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&digest)]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60));

        for _ in 0..3 {
            let grant = cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("la cache responde");
            assert!(grant.expect("hay concesion").is_active_at(Utc::now()));
        }
        assert_eq!(fake.the_reads(), 1, "dos de tres lecturas fueron por cache");
    }

    /// AC de seguridad: una revocacion por esta instancia se nota en la
    /// peticion siguiente, sin esperar al ttl.
    #[tokio::test]
    async fn a_revocation_takes_effect_on_the_next_request() {
        let digest = the_digest_of("revoked bytes");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&digest)]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60));

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("la primera concede")
                .is_some()
        );

        cached
            .revoke_digest(
                "acme",
                "trusted-repo",
                &digest,
                "gatekeeper",
                "CVE-2026-0001",
                Utc::now(),
            )
            .await
            .expect("la revocacion pasa por la cache");

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("la segunda pregunta")
                .is_none(),
            "revocado, la cache no lo recuerda"
        );
    }

    /// La revocacion por decision barre todas sus claves: la decision
    /// promovio varios digests de golpe.
    #[tokio::test]
    async fn revoking_a_decision_clears_every_digest_it_granted() {
        let first = the_digest_of("first");
        let second = the_digest_of("second");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&first), a_grant(&second)]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60));

        for digest in [&first, &second] {
            cached
                .active_grant("acme", "trusted-repo", digest, Utc::now())
                .await
                .expect("ambos concedidos");
        }
        cached
            .revoke_decision(
                "acme",
                "trusted-repo",
                "dec-1",
                "gatekeeper",
                "policy",
                Utc::now(),
            )
            .await
            .expect("revocar la decision");

        for digest in [&first, &second] {
            assert!(
                cached
                    .active_grant("acme", "trusted-repo", digest, Utc::now())
                    .await
                    .expect("la cache vuelve al libro")
                    .is_none()
            );
        }
    }

    /// AC1: la clave es el digest. Mover la etiqueta a otros bytes
    /// pregunta por una clave que la cache no tiene: la decision de ayer
    /// sobre `sha256:X` no dice nada sobre `sha256:Y`.
    #[tokio::test]
    async fn a_moved_tag_does_not_inherit_the_cached_decision() {
        let old_bytes = the_digest_of("the bytes the tag had");
        let new_bytes = the_digest_of("the bytes the tag moved to");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&old_bytes)]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60));

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &old_bytes, Utc::now())
                .await
                .expect("lo viejo esta concedido")
                .is_some()
        );

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &new_bytes, Utc::now())
                .await
                .expect("lo nuevo se pregunta al libro")
                .is_none(),
            "la cache no presta su si a otros bytes"
        );
    }

    /// Lo negado se pregunta cada vez: promover algo nuevo por otra
    /// instancia se sirve en la peticion siguiente.
    #[tokio::test]
    async fn negatives_are_never_cached() {
        let digest = the_digest_of("not yet admitted");
        let fake = std::sync::Arc::new(FakeLedger::new(Vec::new()));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(60));

        for _ in 0..2 {
            assert!(
                cached
                    .active_grant("acme", "trusted-repo", &digest, Utc::now())
                    .await
                    .expect("el libro dice que no")
                    .is_none()
            );
        }
        assert_eq!(fake.the_reads(), 2, "cada no vuelve a preguntar");

        // Y cuando otra instancia promove, aqui se ve sin ttl de por medio.
        fake.grant(&[a_grant(&digest)])
            .await
            .expect("promocion ajena");
        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("la promocion se sirve")
                .is_some()
        );
    }

    /// El ttl es el techo de desactualizacion: caducada la entrada, la
    /// cache vuelve a preguntar, y si el libro murio entre tanto, la
    /// pregunta se hace y falla —el fallo nunca es un permiso.
    #[tokio::test]
    async fn an_expired_entry_asks_the_book_again() {
        let digest = the_digest_of("expired admission");
        let fake = std::sync::Arc::new(FakeLedger::new(vec![a_grant(&digest)]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::ZERO);

        for _ in 0..2 {
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("con ttl cero nada es fresco");
        }
        assert_eq!(fake.the_reads(), 2, "el ttl expirado iguala al libro vivo");
    }

    /// El libro que se deja bloquear dentro de la lectura, para fijar la
    /// carrera entre una lectura iniciada antes de la revocacion y su
    /// insercion posterior.
    struct BlockyLedger {
        grants: tokio::sync::Mutex<Vec<TrustGrant>>,
        entered: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    }

    impl BlockyLedger {
        fn new(grants: Vec<TrustGrant>) -> Self {
            Self {
                grants: tokio::sync::Mutex::new(grants),
                entered: tokio::sync::Mutex::new(None),
                release: tokio::sync::Mutex::new(None),
            }
        }

        /// Arma una barrera: la proxima lectura avisa y espera.
        async fn arm(
            &self,
        ) -> (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ) {
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            *self.entered.lock().await = Some(entered_tx);
            *self.release.lock().await = Some(release_rx);
            (entered_rx, release_tx)
        }
    }

    #[async_trait]
    impl TrustLedger for BlockyLedger {
        async fn grant(&self, grants: &[TrustGrant]) -> Result<(), CatalogError> {
            *self.grants.lock().await = grants.to_vec();
            Ok(())
        }

        async fn revoke_decision(
            &self,
            account: &str,
            _repository: &str,
            decision_id: &str,
            _actor: &str,
            _reason: &str,
            at: DateTime<Utc>,
        ) -> Result<u64, CatalogError> {
            let mut revoked = 0;
            for grant in self.grants.lock().await.iter_mut() {
                if grant.account == account
                    && grant.decision_id == decision_id
                    && grant.revoked_at.is_none()
                {
                    grant.revoked_at = Some(at);
                    revoked += 1;
                }
            }
            Ok(revoked)
        }

        async fn revoke_decision_for_account(
            &self,
            account: &str,
            decision_id: &str,
            actor: &str,
            reason: &str,
            at: DateTime<Utc>,
        ) -> Result<u64, CatalogError> {
            self.revoke_decision(account, "trusted-repo", decision_id, actor, reason, at)
                .await
        }

        async fn revoke_digest(
            &self,
            _account: &str,
            _repository: &str,
            digest: &ContentDigest,
            _actor: &str,
            _reason: &str,
            at: DateTime<Utc>,
        ) -> Result<u64, CatalogError> {
            let mut revoked = 0;
            for grant in self.grants.lock().await.iter_mut() {
                if &grant.digest == digest && grant.revoked_at.is_none() {
                    grant.revoked_at = Some(at);
                    revoked += 1;
                }
            }
            Ok(revoked)
        }

        async fn active_grant(
            &self,
            _account: &str,
            _repository: &str,
            digest: &ContentDigest,
            at: DateTime<Utc>,
        ) -> Result<Option<TrustGrant>, CatalogError> {
            // La foto se toma **antes** de la barrera: una lectura del
            // libro iniciada antes de la revocacion ve el estado que
            // habia cuando empezo, y el problema es donde la envoltora
            // con cache inserta esa foto tardia.
            let snapshot = {
                let grants = self.grants.lock().await;
                grants
                    .iter()
                    .find(|grant| &grant.digest == digest && grant.is_active_at(at))
                    .cloned()
            };
            if let Some(sender) = self.entered.lock().await.take() {
                let _ = sender.send(());
                if let Some(waiter) = self.release.lock().await.take() {
                    let _ = waiter.await;
                }
            }
            Ok(snapshot)
        }

        async fn grants_of(
            &self,
            _account: &str,
            _repo: &str,
        ) -> Result<Vec<TrustGrant>, CatalogError> {
            Ok(self.grants.lock().await.clone())
        }
    }

    /// AC1: la carrera reproducida con barreras deterministas. Una
    /// lectura iniciada **antes** de la revocacion no puede reabrir la
    /// concesion en la cache despues de su confirmacion.
    #[tokio::test]
    async fn a_read_started_before_a_confirmed_revoke_never_admits_again() {
        let digest = the_digest_of("revoked mid-read");
        let book = std::sync::Arc::new(BlockyLedger::new(vec![a_grant(&digest)]));
        let cached = std::sync::Arc::new(CachedTrustLedger::new(
            book.clone(),
            Duration::from_secs(60),
        ));

        let (entered, release) = book.arm().await;
        let reader = {
            let cached = cached.clone();
            let digest = digest.clone();
            tokio::spawn(async move {
                cached
                    .active_grant("acme", "trusted-repo", &digest, Utc::now())
                    .await
            })
        };
        entered.await.expect("the read reached the book");

        cached
            .revoke_digest(
                "acme",
                "trusted-repo",
                &digest,
                "gatekeeper",
                "CVE-2026-0001",
                Utc::now(),
            )
            .await
            .expect("the revoke is confirmed while the read is in flight");
        release.send(()).expect("the blocked read is released");

        let in_flight = reader.await.expect("the reader task finished");
        assert!(
            in_flight.expect("the in-flight read answered").is_some(),
            "la peticion en vuelo se atiende con su snapshot"
        );

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("the next query goes to the book")
                .is_none(),
            "AC1: ninguna consulta iniciada tras la confirmacion reabre la concesion vieja"
        );
    }

    /// La misma carrera con una re-concesion: una concesion nueva escrita
    /// durante la lectura vieja no puede quedar tapada por la insercion
    /// tardia de la decision anterior.
    #[tokio::test]
    async fn a_regrant_written_mid_read_is_not_buried_by_the_stale_insert() {
        let digest = the_digest_of("regranted mid-read");
        let book = std::sync::Arc::new(BlockyLedger::new(vec![a_grant(&digest)]));
        let cached = std::sync::Arc::new(CachedTrustLedger::new(
            book.clone(),
            Duration::from_secs(60),
        ));

        let (entered, release) = book.arm().await;
        let reader = {
            let cached = cached.clone();
            let digest = digest.clone();
            tokio::spawn(async move {
                cached
                    .active_grant("acme", "trusted-repo", &digest, Utc::now())
                    .await
            })
        };
        entered.await.expect("the read reached the book");

        let mut fresh = a_grant(&digest);
        fresh.decision_id = "dec-2".to_string();
        cached
            .grant(&[fresh])
            .await
            .expect("the regrant lands mid-read");
        release.send(()).expect("the blocked read is released");
        reader
            .await
            .expect("the reader task finished")
            .expect("the in-flight read answered");

        let found = cached
            .active_grant("acme", "trusted-repo", &digest, Utc::now())
            .await
            .expect("the ledger answers");
        assert_eq!(
            found.expect("the fresh grant admits").decision_id,
            "dec-2",
            "la decision nueva no queda enterrada por la insercion vieja"
        );
    }

    /// AC3: la revocacion de una cuenta no desmonta las entradas ni el
    /// progreso de las lecturas en vuelo de otra cuenta.
    #[tokio::test]
    async fn revoking_one_account_leaves_another_in_flight_untouched() {
        let mine = a_grant(&the_digest_of("mine"));
        let mut theirs = a_grant(&the_digest_of("theirs"));
        theirs.account = "globex".to_string();
        let book = std::sync::Arc::new(BlockyLedger::new(vec![mine, theirs.clone()]));
        let cached = std::sync::Arc::new(CachedTrustLedger::new(
            book.clone(),
            Duration::from_secs(60),
        ));

        let (entered, release) = book.arm().await;
        let theirs_digest = theirs.digest.clone();
        let reader = {
            let cached = cached.clone();
            let theirs_digest = theirs_digest.clone();
            tokio::spawn(async move {
                cached
                    .active_grant("globex", "trusted-repo", &theirs_digest, Utc::now())
                    .await
            })
        };
        entered.await.expect("the globex read is in flight");

        cached
            .revoke_decision(
                "acme",
                "trusted-repo",
                "dec-1",
                "gatekeeper",
                "policy",
                Utc::now(),
            )
            .await
            .expect("the acme revoke is confirmed");
        release.send(()).expect("the globex read is released");
        let _ = reader.await.expect("the reader task finished");

        assert!(
            cached
                .active_grant("globex", "trusted-repo", &theirs_digest, Utc::now())
                .await
                .expect("globex still admitted")
                .is_some(),
            "AC3: revocar en acme no invalida a globex"
        );
    }

    /// AC2, y la separacion que el contrato exige: la cache acorta el
    /// viaje, no alarga la vida. Una entrada con ttl de una hora no
    /// sirve un grant cuya vigencia caduco.
    #[tokio::test]
    async fn the_cache_does_not_extend_the_validity_of_a_grant() {
        let digest = the_digest_of("short lived admission");
        let mut grant = a_grant(&digest);
        grant.valid_until = Some(Utc::now() + chrono::Duration::minutes(5));
        let fake = std::sync::Arc::new(FakeLedger::new(vec![grant]));
        let cached = CachedTrustLedger::new(fake.clone(), Duration::from_secs(3600));

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, Utc::now())
                .await
                .expect("viva, se sirve")
                .is_some()
        );

        let later = Utc::now() + chrono::Duration::hours(1);
        assert!(
            cached
                .active_grant("acme", "trusted-repo", &digest, later)
                .await
                .expect("caducada, el libro lo dice")
                .is_none(),
            "la cache no presta vida a un grant caducado"
        );
        assert_eq!(fake.the_reads(), 2, "y la caducidad se comprueba de nuevo");
    }
}
