//! HSP-192: el libro coordinado de la GC sobre el motor Dgraph real.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` como todos los IT del stack. Lo que
//! prueba, contra el servidor vivo h186 (TLS + ACL), con claves de scope
//! propias por ejecución (`it192<tag><nonce>`, nunca un censo global —
//! en el mismo alpha hay slices OCI y de simulación en vuelo):
//!
//! - **AC1** el claim es una guarda CAS DURABLE sobre la fila del lease
//!   (no un contador en memoria): un claim ajeno vigente ocupa el scope
//!   (`Conflict`), el take-over del lease vencido sube generación y da
//!   token nuevo, y un zombie con la generación vieja queda
//!   `PreconditionFailed` en TODA operación de pasada — marcar,
//!   reconfirmar, registrar, renovar o cerrar. Dos clientes sobre
//!   scopes disjoint conviven en el mismo alpha;
//! - **AC2/entrega 2** la marca y el tombstone llevan la guarda de
//!   referencias EN la misma transacción: un candidato con raíz viva
//!   aborta la marca y no deja rastro, y el veredicto se decide por
//!   relectura post-commit (HSP-205), nunca por "el upsert devolvió OK";
//! - **AC3** el censo va por MEMBRESÍA de la cuenta (paginado, cursor =
//!   digest), y las superficies globales (`content_roots`,
//!   `pending_run`) fallan cerradas por HSP-220;
//! - y la pasada coordinada completa (`CoordinatedPass` +
//!   `FileBlobStore` bajo `target/uat/hsp192/`) barre el huérfano y no
//!   toca lo publicado, con las páginas contadas.
//!
//! Corre serializada (`--test-threads=1`): el alpha es compartido.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use hodei_artifacts_api::gc::{CoordinatedPass, GcService, PassAction, PassConfig};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogError, CatalogWriter, ContentLedger, DeletionState, DigestScope,
    GcDeletion, GcRun, PackageCoordinates, Publication, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, RootKind, RunCandidate, RunState, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::{BlobStore, FileBlobStore};

/// El directorio de evidencia de HSP-192: los object stores de prueba
/// viven aqui y NO se borran nunca (convención del workspace).
const UAT_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/uat/hsp192");

const GRACE: Duration = Duration::ZERO;

fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let ca = std::env::var("HODEI_GRAPH_IT_CA").unwrap_or_default();
    let server_name = std::env::var("HODEI_GRAPH_IT_SERVER_NAME").unwrap_or_default();
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    let mut text = format!(
        "endpoint = \"{endpoint}\"\n\n[tls]\nenabled = {enabled}\nca_cert_path = \"{ca}\"\n",
        enabled = !ca.is_empty()
    );
    if !server_name.is_empty() {
        text.push_str(&format!("server_name = \"{server_name}\"\n"));
    }
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

/// Una conexión nueva y autenticada. `None` sin entorno: el IT está gated.
async fn fresh_client() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    Some(Arc::new(client))
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// Un scope (la cuenta) por prueba y por ejecución: ninguna corrida se
/// encuentra con los leases de otra, y el censo por membresía solo ve
/// lo de esta prueba.
fn account(tag: &str) -> String {
    format!("it192{}{}", tag, &nonce()[8..])
}

async fn open_store(client: Arc<GraphClient>) -> Arc<DgraphCatalogStore> {
    Arc::new(
        DgraphCatalogStore::open(client.clone(), Arc::new(AllowAllAuthorizer))
            .await
            .expect("the dgraph catalog store opens"),
    )
}

/// El libro coordinado: la misma fila del catálogo, vista como ledger.
fn ledger(store: &Arc<DgraphCatalogStore>) -> Arc<dyn ContentLedger> {
    Arc::clone(store) as Arc<dyn ContentLedger>
}

fn a_descriptor(account: &str, repo_key: &str) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, repo_key),
        account: account.to_string(),
        repo_key: repo_key.to_string(),
        format: RepositoryFormat::Raw,
        mode: RepositoryMode::Hosted,
        config: RepositoryConfig::new(DigestScope::Global),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn a_publication(account: &str, content: &[u8], slot: &str) -> Publication {
    Publication {
        repository: "raw".to_string(),
        asset_path: format!("app/{slot}.bin"),
        // La version del fixture es el slot: dos publicaciones del
        // mismo paquete colisionarian en `pkg@1.0.0` (el catalogo raw
        // es inmutable por version) y la segunda daria Conflict.
        coordinates: PackageCoordinates {
            package_name: "pkg".to_string(),
            package_version: slot.to_string(),
        },
        digest: ContentDigest::of_bytes(content),
        size_bytes: content.len() as u64,
        tags: Vec::new(),
        actor: format!("hrn:hodei:iam:{account}:User/alice"),
        correlation_id: format!("corr-hsp192-{account}-{slot}"),
        overwrite: false,
        precondition: None,
        subject: None,
    }
}

fn orphan_digest(label: &str) -> ContentDigest {
    ContentDigest::of_bytes(format!("orphan-{label}-{}", nonce()).as_bytes())
}

/// Un run_id por prueba y por ejecución: la clave del run es el id, y
/// dos corridas no deben pisarse la marca durable.
fn run_id(tag: &str) -> String {
    format!("gc-{tag}-{}", &nonce()[8..])
}

fn run_candidate(digest: ContentDigest) -> RunCandidate {
    RunCandidate { digest, length: 12 }
}

fn a_run(scope: &str, run_id: &str, generation: u64, candidates: Vec<RunCandidate>) -> GcRun {
    GcRun {
        id: run_id.to_string(),
        state: RunState::Marked,
        grace_secs: GRACE.as_secs(),
        actor: scope.to_string(),
        candidates,
        marked_at: Utc::now(),
        deleted: 0,
        bytes_freed: 0,
        swept_at: None,
        scope: Some(scope.to_string()),
        generation: Some(generation),
    }
}

fn a_deletion(digest: &ContentDigest, scope: &str, run_id: &str) -> GcDeletion {
    GcDeletion {
        digest: digest.clone(),
        length: 12,
        run_id: run_id.to_string(),
        actor: "worker".to_string(),
        account: scope.to_string(),
        state: DeletionState::Intended,
        at: Utc::now(),
        marker: Some(format!("mtime:test:{}", digest.value)),
    }
}

// ---------------------------------------------------------------------------
// AC1: el claim durable CAS y el zombie con la generación vencida.
// ---------------------------------------------------------------------------

/// El claim vive en la autoridad: un claim ajeno vigente ocupa el scope,
/// el mismo owner renueva SIN cambiar token ni generación, y dos scopes
/// disjoint conviven sobre el mismo alpha con dos clientes distintos.
#[tokio::test]
async fn the_claim_is_a_durable_cas_and_a_live_foreign_lease_busy_the_scope() {
    let Some(client_a) = fresh_client().await else {
        return;
    };
    let client_b = fresh_client().await.expect("the second replica connects");
    let store_a = open_store(client_a).await;
    let store_b = open_store(client_b).await;

    let scope = account("claim");
    let foreign = account("foreign");
    let now = Utc::now();

    assert!(ledger(&store_a).supports_fenced_pass());

    // Claim inicial: generación 1, durable y releible post-commit.
    let lease = ledger(&store_a)
        .claim_gc_pass(&scope, "worker-a", Duration::from_secs(30), now)
        .await
        .expect("the first claim");
    assert_eq!(lease.generation, 1);
    let back = ledger(&store_a)
        .current_gc_lease(&scope, Utc::now())
        .await
        .expect("the lease is readable")
        .expect("the lease is live");
    assert_eq!(back.token, lease.token);
    assert_eq!(back.generation, lease.generation);

    // Otro owner con lease vigente: el scope está OCUPADO (Conflict),
    // decidido por la guarda del servidor, no por una pre-lectura.
    let busy = ledger(&store_b)
        .claim_gc_pass(&scope, "worker-b", Duration::from_secs(30), Utc::now())
        .await;
    assert!(
        matches!(busy, Err(CatalogError::Conflict { .. })),
        "a live foreign lease must busy the scope: {busy:?}"
    );

    // El mismo owner renueva: misma generación, mismo token.
    let renewed = ledger(&store_a)
        .claim_gc_pass(&scope, "worker-a", Duration::from_secs(30), Utc::now())
        .await
        .expect("the owner renews its own live lease");
    assert_eq!(renewed.generation, lease.generation);
    assert_eq!(renewed.token, lease.token);

    // Scopes disjoint sobre el MISMO alpha con otro cliente: no se
    // pisar (la clave del lease es por scope).
    let other = ledger(&store_b)
        .claim_gc_pass(&foreign, "worker-b", Duration::from_secs(30), Utc::now())
        .await
        .expect("a disjoint scope claims independently");
    assert_eq!(other.generation, 1);
    assert_ne!(other.token, lease.token);
}

/// El zombie de la generación vencida NO puede seguir borrando: marcar,
/// reconfirmar, registrar, renovar y cerrar fallan con el fence, y el
/// rastro del blob que el zombie pretendía barrer no existe. La
/// autoridad (la fila del lease) decidió el take-over, no el worker.
#[tokio::test]
async fn a_zombie_with_an_expired_lease_cannot_keep_sweeping_the_new_generation() {
    let Some(client_a) = fresh_client().await else {
        return;
    };
    let client_b = fresh_client().await.expect("the second replica connects");
    let store_a = open_store(client_a).await;
    let store_b = open_store(client_b).await;
    let (scope_a, ledger_a, ledger_b) = {
        let scope = account("zombie");
        (scope, ledger(&store_a), ledger(&store_b))
    };

    // Generación 1 con un lease que caduca solo.
    let zombie = ledger_a
        .claim_gc_pass(&scope_a, "worker-a", Duration::from_secs(2), Utc::now())
        .await
        .expect("the zombie claims");
    assert_eq!(zombie.generation, 1);
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    assert!(
        ledger_a
            .current_gc_lease(&scope_a, Utc::now())
            .await
            .expect("the lease read")
            .is_none(),
        "an expired lease is no longer live"
    );

    // Take-over real: otro worker, generación nueva y token NUEVO.
    let fresh = ledger_b
        .claim_gc_pass(&scope_a, "worker-b", Duration::from_secs(30), Utc::now())
        .await
        .expect("the take-over of an expired claim");
    assert_eq!(fresh.generation, 2);
    assert_ne!(fresh.token, zombie.token);

    // El zombie presenta su token viejo en CADA operación de pasada.
    let bait = orphan_digest("zombie-bait");
    let zombie_run = run_id("zombie");
    let marked = ledger_a
        .mark_run_fenced(
            &scope_a,
            &zombie,
            &a_run(&scope_a, &zombie_run, 1, vec![run_candidate(bait.clone())]),
            std::slice::from_ref(&bait),
        )
        .await;
    assert!(
        matches!(marked, Err(CatalogError::PreconditionFailed { .. })),
        "the zombie cannot write the mark: {marked:?}"
    );
    assert!(matches!(
        ledger_a.mark_of_run(&scope_a, &zombie, &zombie_run).await,
        Err(CatalogError::PreconditionFailed { .. })
    ));
    assert!(matches!(
        ledger_a
            .record_deletion_fenced(&scope_a, &zombie, &a_deletion(&bait, &scope_a, &zombie_run))
            .await,
        Err(CatalogError::PreconditionFailed { .. })
    ));
    assert!(matches!(
        ledger_a
            .renew_gc_lease(
                &scope_a,
                &zombie,
                "worker-a",
                Duration::from_secs(30),
                Utc::now()
            )
            .await,
        Err(CatalogError::PreconditionFailed { .. })
    ));
    assert!(matches!(
        ledger_a
            .complete_scoped_run(&scope_a, &zombie, &zombie_run, 1, 12, 0, Utc::now())
            .await,
        Err(CatalogError::PreconditionFailed { .. })
    ));

    // Y el veredicto final es la relectura: el zombie NO dejó marca ni
    // rastro — no pudo seguir borrando la generación nueva.
    assert!(
        ledger_a
            .pending_scoped_run(&scope_a, Utc::now())
            .await
            .expect("the pending read")
            .is_none(),
        "the zombie left no durable mark"
    );
    assert!(
        ledger_a
            .deletion_of(&bait)
            .await
            .expect("the tombstone read")
            .is_none(),
        "the zombie left no tombstone"
    );

    // El holder vigente sí opera bajo su token nuevo.
    let live_bait = orphan_digest("holder");
    ledger_b
        .mark_run_fenced(
            &scope_a,
            &fresh,
            &a_run(
                &scope_a,
                &run_id("holder"),
                2,
                vec![run_candidate(live_bait.clone())],
            ),
            &[live_bait],
        )
        .await
        .expect("the live holder marks");
}

// ---------------------------------------------------------------------------
// AC3 + entrega 2: censo por membresía y guardas en la misma transacción.
// ---------------------------------------------------------------------------

/// El censo va por MEMBRESÍA de la cuenta y por páginas acotadas: nunca
/// se materializa el grafo, y las superficies globales fallan cerradas
/// (HSP-220). La cuenta ajena no ve ni una raíz.
#[tokio::test]
async fn the_census_pages_by_membership_and_the_global_surfaces_fail_closed() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let store = open_store(client).await;
    let scope = account("census");
    store
        .create_repository(&scope, a_descriptor(&scope, "raw"))
        .await
        .expect("the repository");

    let mut published = Vec::new();
    for slot in ["one", "two", "three"] {
        let content = format!("census-{slot}-{scope}").into_bytes();
        let publication = a_publication(&scope, &content, slot);
        published.push(publication.digest.clone());
        store
            .publish(&scope, publication)
            .await
            .expect("the publication");
    }
    // Cada publicacion raw deja TRES raices con digest (asset, version y
    // reference): el censo por membresia las ve todas, nueve en total.
    let expected_roots = published.len() * 3;

    // Las superficies globales no existen en esta autoridad: un censo
    // materializado sería el scan prohibido.
    assert!(matches!(
        ledger(&store).content_roots().await,
        Err(CatalogError::Unavailable { .. })
    ));
    assert!(matches!(
        ledger(&store).pending_run().await,
        Err(CatalogError::Unavailable { .. })
    ));

    // Paginado con cota 2 sobre 9 raices: mas de una pagina, cursor =
    // digest, y entre todas cubren el censo propio EXACTAMENTE una vez
    // (sin materializar y sin repetirse en el cruce de familias).
    let mut seen: Vec<(RootKind, String, ContentDigest)> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0usize;
    loop {
        let page = ledger(&store)
            .root_page(&scope, cursor.as_deref(), 2)
            .await
            .expect("the census page");
        pages += 1;
        assert!(!page.roots.is_empty(), "a bounded page must carry rows");
        // La cota es dura sobre las filas DISTINTAS del borde: los
        // empates del digest de corte se completan en la misma pagina
        // (son la misma clave de censo vista por otra familia).
        let distinct_at_edge: Vec<String> = page
            .roots
            .iter()
            .map(|root| root.digest.to_string())
            .collect();
        let mut edge = distinct_at_edge.clone();
        edge.dedup();
        assert!(
            edge.len() <= 2,
            "the page crosses at most `limit` distinct digests: {edge:?}"
        );
        for root in &page.roots {
            assert_eq!(root.account, scope, "the census is by membership");
            seen.push((root.kind, root.subject.clone(), root.digest.clone()));
        }
        if !page.has_more {
            break;
        }
        cursor = Some(
            page.roots
                .last()
                .expect("a full page has a cursor")
                .digest
                .to_string(),
        );
        assert!(pages < 32, "the pagination must converge");
    }
    assert!(pages > 1, "nine roots with limit 2 cannot fit one page");
    assert_eq!(seen.len(), expected_roots, "the census saw every root once");
    let mut per_digest: Vec<(String, usize)> = seen
        .iter()
        .map(|(_, _, digest)| (digest.value.clone(), 1usize))
        .collect();
    per_digest.sort_by(|a, b| a.0.cmp(&b.0));
    let mut merged: Vec<(String, usize)> = Vec::new();
    for (value, one) in per_digest {
        match merged.last_mut() {
            Some((previous, count)) if *previous == value => *count += one,
            _ => merged.push((value, one)),
        }
    }
    assert!(
        merged.iter().all(|(_, count)| *count == 3),
        "each publication contributes exactly its three roots: {merged:?}"
    );
    for digest in &published {
        let mut mine: Vec<_> = seen
            .iter()
            .filter(|(_, _, found)| found == digest)
            .map(|(kind, _, _)| *kind)
            .collect();
        // El orden dentro del mismo digest es el de la clave de fila,
        // no el de la familia: se compara como conjunto.
        mine.sort_by_key(|kind| kind.as_str().to_owned());
        assert_eq!(
            mine,
            [RootKind::Asset, RootKind::Reference, RootKind::Version],
            "each publication contributes its three membership roots"
        );
    }

    // Una cuenta ajena no ve NADA: la pagina va por membresía.
    let foreign = account("census-foreign");
    let page = ledger(&store)
        .root_page(&foreign, None, 10)
        .await
        .expect("the foreign page");
    assert!(page.roots.is_empty());
    assert!(!page.has_more);

    // La sonda de referencias frescas: lo publicado está vivo, el
    // huerfano no.
    assert!(
        ledger(&store)
            .live_reference_count(&published[0])
            .await
            .expect("the reference probe")
            > 0
    );
    assert_eq!(
        ledger(&store)
            .live_reference_count(&orphan_digest("uncatalogued"))
            .await
            .expect("the reference probe"),
        0
    );
}

/// Las guardas viajan EN la transacción y el veredicto es la relectura
/// post-commit: un candidato con raíz viva aborta la marca; un tombstone
/// sobre contenido referenciado no se escribe (aunque el lease sea
/// vigente), y lo que la autoridad dice al leer es lo único que vale.
#[tokio::test]
async fn the_reference_guard_lives_in_the_mark_and_the_verdict_is_the_post_commit_read() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let store = open_store(client).await;
    let scope = account("guard");
    store
        .create_repository(&scope, a_descriptor(&scope, "raw"))
        .await
        .expect("the repository");
    let content = format!("guarded-{scope}").into_bytes();
    let publication = a_publication(&scope, &content, "guarded");
    let referenced = publication.digest.clone();
    store
        .publish(&scope, publication)
        .await
        .expect("the publication");

    let lease = ledger(&store)
        .claim_gc_pass(&scope, "worker", Duration::from_secs(30), Utc::now())
        .await
        .expect("the claim");
    let free = orphan_digest("truly-orphan");

    // Marca con un candidato referenciado: la guarda aborta la marca
    // ENTERA, no se cuela el huérfano a medias.
    let guarded_run = run_id("guarded");
    let marked = ledger(&store)
        .mark_run_fenced(
            &scope,
            &lease,
            &a_run(
                &scope,
                &guarded_run,
                lease.generation,
                vec![
                    run_candidate(free.clone()),
                    run_candidate(referenced.clone()),
                ],
            ),
            &[free.clone(), referenced.clone()],
        )
        .await;
    assert!(
        matches!(marked, Err(CatalogError::PreconditionFailed { .. })),
        "a referenced candidate aborts the mark: {marked:?}"
    );

    // El tombstone de lo referenciado, CON lease vigente: la guarda de
    // referencias lo bloquea en el propio upsert y la relectura dice
    // que no quedó — veredicto post-commit, no fe en el commit.
    let wrote = ledger(&store)
        .record_deletion_fenced(
            &scope,
            &lease,
            &a_deletion(&referenced, &scope, &guarded_run),
        )
        .await;
    assert!(
        matches!(wrote, Err(CatalogError::Unavailable { .. })),
        "the tombstone of live content must not land: {wrote:?}"
    );
    assert!(
        ledger(&store)
            .deletion_of(&referenced)
            .await
            .expect("the tombstone read")
            .is_none()
    );

    // El candidato libre sí: la marca pasa, y el rastro se escribe y se
    // confirma bajo el token vigente.
    let clean_run = run_id("clean");
    let marked = ledger(&store)
        .mark_run_fenced(
            &scope,
            &lease,
            &a_run(
                &scope,
                &clean_run,
                lease.generation,
                vec![run_candidate(free.clone())],
            ),
            std::slice::from_ref(&free),
        )
        .await
        .expect("the clean mark");
    assert_eq!(marked.len(), 1);
    ledger(&store)
        .record_deletion_fenced(&scope, &lease, &a_deletion(&free, &scope, &clean_run))
        .await
        .expect("the intention of the orphan lands");
    let trace = ledger(&store)
        .deletion_of(&free)
        .await
        .expect("the tombstone read")
        .expect("the tombstone is there after commit");
    assert_eq!(trace.state, DeletionState::Intended);

    // La marca pendiente es DURABLE: otro cliente del mismo scope la ve
    // bajo la generación vigente (la autoridad manda, no el proceso).
    let pending = ledger(&store)
        .pending_scoped_run(&scope, Utc::now())
        .await
        .expect("the pending read");
    assert_eq!(
        pending.map(|run| run.id).as_deref(),
        Some(clean_run.as_str())
    );
}

// ---------------------------------------------------------------------------
// La pasada coordinada completa sobre la autoridad real + el CAS de ficheros.
// ---------------------------------------------------------------------------

/// El plano de arriba en acción: `CoordinatedPass` sobre el libro Dgraph
/// y un `FileBlobStore` bajo `target/uat/hsp192/` barre SOLO el huerfano,
/// no toca lo publicado, y el censo va contado por páginas.
#[tokio::test]
async fn the_coordinated_pass_sweeps_the_orphan_and_leaves_the_referenced_alone() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let store = open_store(client).await;
    let scope = account("sweep");

    let cas_root = std::path::Path::new(UAT_ROOT).join(format!("cas-{scope}"));
    std::fs::create_dir_all(&cas_root).expect("the UAT evidence directory");
    let content = Arc::new(FileBlobStore::new(&cas_root).expect("the file CAS"));

    store
        .create_repository(&scope, a_descriptor(&scope, "raw"))
        .await
        .expect("the repository");
    let mut referenced = Vec::new();
    for slot in ["keep-one", "keep-two", "keep-three"] {
        let bytes = format!("sweep-{slot}-{scope}").into_bytes();
        content
            .put(&bytes)
            .await
            .expect("the blob of a published asset");
        referenced.push(ContentDigest::of_bytes(&bytes));
        store
            .publish(&scope, a_publication(&scope, &bytes, slot))
            .await
            .expect("the publication");
    }
    let orphan = content
        .put(b"orphan sweep bait")
        .await
        .expect("the orphan blob");

    let service = Arc::new(
        GcService::new(
            ledger(&store),
            Arc::clone(&content) as Arc<dyn BlobStore>,
            GRACE,
        )
        .expect("the file store reclaims"),
    );
    let pass = CoordinatedPass::new(
        Arc::clone(&service),
        PassConfig::new(scope.clone(), "worker", Duration::from_secs(30)).with_page_limit(2),
    );

    let report = pass.run().await.expect("the coordinated pass");
    assert_eq!(report.deleted, 1, "{report:?}");
    assert_eq!(report.bytes_freed, b"orphan sweep bait".len() as u64);
    assert_eq!(report.unknown, 0);
    // Nueve raices propias con cota de pagina 2: el censo se recorre
    // en VARIAS paginas acotadas, nunca de golpe. El numero exacto
    // depende de donde caigan los empates del borde, no es la
    // propiedad; la propiedad es que haya mas de una pagina.
    assert!(report.census_pages > 1, "{report:?}");
    assert_eq!(
        report.actions.get(&orphan.to_string()),
        Some(&PassAction::Deleted)
    );
    assert!(
        !content.exists(&orphan).await.expect("the listing"),
        "the orphan is gone"
    );
    for digest in &referenced {
        assert!(
            !report.actions.contains_key(&digest.to_string()),
            "referenced content must not even be a candidate"
        );
        assert!(
            content.exists(digest).await.expect("the listing"),
            "the referenced blob stays"
        );
    }

    // El rastro confirmado en la autoridad: post-efecto, post-commit.
    let trace = ledger(&store)
        .deletion_of(&orphan)
        .await
        .expect("the tombstone read")
        .expect("the sweep left its trace");
    assert_eq!(trace.state, DeletionState::Confirmed);

    // Una segunda pasada no encuentra nada que barrer: el censo es
    // estable y el huérfano ya no está.
    let second = pass.run().await.expect("the second pass");
    assert_eq!(second.marked, 0, "{second:?}");
    assert_eq!(second.deleted, 0);
}
