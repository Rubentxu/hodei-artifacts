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

use std::collections::HashMap;
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
}

impl CachedTrustLedger {
    /// El libro real, con el ttl que debe tener cada entrada.
    pub fn new(inner: std::sync::Arc<dyn TrustLedger>, ttl: Duration) -> Self {
        Self {
            inner,
            ttl,
            admitted: Mutex::new(HashMap::new()),
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
    ) -> Result<Option<TrustGrant>, CatalogError> {
        let key = Self::key(account, repository, digest);
        if let Some(entry) = self.fresh().get(&key) {
            return Ok(Some(entry.grant.clone()));
        }
        let grant = self.inner.active_grant(account, repository, digest).await?;
        // Solo la afirmacion se cachea: la ausencia y el fallo del libro
        // se vuelven a preguntar siempre, porque una revocacion hecha en
        // otra instancia o un libro que vuelve a vivir deben poder
        // negarse —y conceder— en la peticion siguiente.
        if let Some(active) = grant.as_ref().filter(|grant| grant.is_active()) {
            self.fresh().insert(
                key,
                Entry {
                    grant: active.clone(),
                    expires_at: Instant::now() + self.ttl,
                },
            );
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
        ) -> Result<Option<TrustGrant>, CatalogError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self
                .grants
                .lock()
                .await
                .iter()
                .find(|grant| &grant.digest == digest && grant.is_active())
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
                .active_grant("acme", "trusted-repo", &digest)
                .await
                .expect("la cache responde");
            assert!(grant.expect("hay concesion").is_active());
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
                .active_grant("acme", "trusted-repo", &digest)
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
                .active_grant("acme", "trusted-repo", &digest)
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
                .active_grant("acme", "trusted-repo", digest)
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
                    .active_grant("acme", "trusted-repo", digest)
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
                .active_grant("acme", "trusted-repo", &old_bytes)
                .await
                .expect("lo viejo esta concedido")
                .is_some()
        );

        assert!(
            cached
                .active_grant("acme", "trusted-repo", &new_bytes)
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
                    .active_grant("acme", "trusted-repo", &digest)
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
                .active_grant("acme", "trusted-repo", &digest)
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
                .active_grant("acme", "trusted-repo", &digest)
                .await
                .expect("con ttl cero nada es fresco");
        }
        assert_eq!(fake.the_reads(), 2, "el ttl expirado iguala al libro vivo");
    }
}
