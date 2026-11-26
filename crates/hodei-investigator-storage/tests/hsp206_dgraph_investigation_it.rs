//! HSP-206: la investigacion y su frescura sobre la autoridad del grafo.
//!
//! IT condicionado a `HODEI_GRAPH_IT_ENDPOINT` como todos los de la pila.
//! Corre contra el alpha REAL compartido (asserts por MEMBRESIA sobre el
//! tenant del test, nunca sobre conteos globales, HSP-198/219/220) y
//! Lo que prueba:
//!
//! - REPRODUCIBILIDAD: dos lecturas a la misma watermark sellan el MISMO
//!   read-set byte a byte (manifiesto y huella identicos), con las
//!   versiones de los apoyos que el informe cita (HSP-89);
//! - RETIRO DE APOYO: un informe que se apoya en un apoyo retirado es
//!   Stale con la cita de lo que se movio; el informe que no se apoyaba
//!   en el movimiento sigue Current; y un apoyo ANADIDO a una relacion
//!   que el informe leyo NO caduca lo que el informe dijo;
//! - FAIL-CLOSED (AC#2): el corte por presupuesto se DECLARA (Coverage
//!   Partial + OmittedByBudget) y el read-set de una lectura cortada
//!   sigue sellando lo leido; una autoridad muda cierra la lectura con
//!   Unavailable, nunca un snapshot inventado; y un manifiesto que cita
//!   un tipo de relacion incomprensible produce UNKNOWN, nunca un
//!   Current fabricado;
//! - SCOPE: un sujeto de otra cuenta no se consulta: UNKNOWN declarado.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::Utc;
use hodei_evidence::application::projection::ProjectionDelivery;
use hodei_evidence::domain::delta::{
    RelationDelta, RelationDeltaLedger, RelationDeltaSubscription,
};
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, GraphRelationWithdrawal, NodeKind, RelationType,
};
use hodei_evidence::{ContentDigest, ContentRef, ContentStore, EvidenceStore};
use hodei_evidence_storage::{
    DgraphDeltaLedger, DgraphEvidenceStore, DgraphGraphStore, DgraphProjectionFrontier,
    InMemoryContentStore,
};
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
use hodei_investigator::freshness::{ChangeKind, Freshness, GRAPH_SOURCE};
use hodei_investigator::investigator::EvidenceRetriever;
use hodei_investigator::read_set::{ReadKind, ReadSet, ReadStamp};
use hodei_investigator::scope::{
    Coverage, EvidenceReadAuthorizer, FactStatus, RetrievalBudget, RetrievalError, RetrievalScope,
    RetrievedContext,
};
use hodei_investigator_storage::freshness::LedgerFreshness;
use hodei_investigator_storage::{DgraphEvidenceRetriever, GraphSource, InvestigationRetriever};
use kernel::Hrn;

/// La puerta de autorizacion es la del nucleo, ya probada por la
/// referencia (m8, HSP-89): aqui se prueba la lectura, el sello y el
/// veredicto de tres sobre el grafo nativo.
struct EveryoneReads;

#[async_trait]
impl EvidenceReadAuthorizer for EveryoneReads {
    async fn can_read(&self, _actor: &Hrn, _resource: &Hrn) -> Result<(), String> {
        Ok(())
    }
}

/// Una fuente del grafo cuya autoridad no contesta: la watermark no se
/// inventa con cero, y las relaciones que fallan no se entregan como
/// ausencia.
struct SilentSource;

#[async_trait]
impl GraphSource for SilentSource {
    async fn generation(&self, _tenant: &Hrn) -> Result<u64, String> {
        Err("the authority did not answer".to_string())
    }

    async fn relations_from(
        &self,
        _tenant: &Hrn,
        _node: &Hrn,
    ) -> Result<Vec<GraphRelation>, String> {
        Err("the authority did not answer".to_string())
    }

    async fn relations_to(&self, _tenant: &Hrn, _node: &Hrn) -> Result<Vec<GraphRelation>, String> {
        Err("the authority did not answer".to_string())
    }
}

/// El lector de una historia de deltas que no contesta: el veredicto
/// sobre un informe tiene que ser UNKNOWN, nunca un Current calculado
/// sobre el silencio.
struct SilentLedger;

#[async_trait]
impl RelationDeltaLedger for SilentLedger {
    async fn append(&self, _deltas: &[RelationDelta]) -> Result<Option<u64>, String> {
        Err("this ledger only fails".to_string())
    }

    async fn pending(
        &self,
        _subscription: &RelationDeltaSubscription,
        _after_version: u64,
        _quota: usize,
    ) -> Result<(Vec<(u64, RelationDelta)>, u64), String> {
        Err("the story did not answer".to_string())
    }

    async fn version_of(&self, _tenant: &Hrn) -> Result<u64, String> {
        Err("the story did not answer".to_string())
    }
}

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

async fn connect() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    let report = ensure_ready(&client)
        .await
        .expect("the versioned schema boots");
    eprintln!(
        "schema boot: decision={:?} binary_version={} stored_version={:?}",
        report.decision, report.binary_version, report.stored_version
    );
    Some(Arc::new(client))
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// Un tenant por test y por ejecucion: en el alpha compartido cada
/// slice deja su huella, y las aserciones son de membresia sobre el
/// propio, no de totales globales.
fn fresh_tenant(tag: &str) -> Hrn {
    let account = format!("it{tag}{}", &nonce()[7..]);
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:Artifact/hsp206"
    ))
    .expect("a valid tenant HRN")
}

fn hrn_of(tenant: &Hrn, kind: &str, name: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{}:{kind}/{name}",
        tenant.account_id()
    ))
    .expect("a valid HRN")
}

fn kind_label(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Repository => "Repository",
        NodeKind::SourceRevision => "SourceRevision",
        NodeKind::Build => "Build",
        NodeKind::Artifact => "Artifact",
        NodeKind::Deployment => "Deployment",
        NodeKind::Evidence => "Evidence",
        NodeKind::Component => "Component",
        NodeKind::Vulnerability => "Vulnerability",
    }
}

fn node(tenant: &Hrn, kind: NodeKind, name: &str) -> GraphNode {
    GraphNode::new(
        hrn_of(tenant, kind_label(&kind), name),
        tenant.clone(),
        kind,
        Utc::now(),
    )
}

fn relation(
    tenant: &Hrn,
    kind: RelationType,
    from: &Hrn,
    to: &Hrn,
    evidence: &str,
) -> GraphRelation {
    GraphRelation::new(
        kind,
        from.clone(),
        to.clone(),
        tenant.clone(),
        EvidenceId::new(evidence),
        Utc::now(),
    )
}

fn withdrawal(
    tenant: &Hrn,
    kind: RelationType,
    from: &Hrn,
    to: &Hrn,
    evidence: &str,
) -> GraphRelationWithdrawal {
    GraphRelationWithdrawal::new(
        kind,
        from.clone(),
        to.clone(),
        tenant.clone(),
        EvidenceId::new(evidence),
        Utc::now(),
    )
}

/// El registro de evidencia del mundo: contenido por digest, sujeto por
/// HRN. El digest es funcion del texto, y por eso el sello de evidencia
/// es el MISMO string en los dos motores.
fn evidence_record(tenant: &Hrn, id: &str, subject: &Hrn, text: &str) -> EvidenceRecord {
    let digest = ContentDigest::of_bytes(text.as_bytes());
    EvidenceRecord::builder(EvidenceId::new(id), tenant.clone())
        .evidence_type(EvidenceType::Sbom)
        .subjects(vec![subject.clone()])
        .source(SourceRef::by_external_id(format!("ext-{id}")))
        .content(ContentRef::by_digest(digest))
        .observed_at(Utc::now())
        .recorded_at(Utc::now())
        .verification(EvidenceVerification::verified("dsse"))
        .normalization(Normalization::new("cyclonedx", "1.0.0"))
        .producer(Producer::new("connector", "hsp206-it").with_version("1.0.0"))
        .build()
}

fn the_actor(tenant: &Hrn) -> Hrn {
    hrn_of(tenant, "User", "alice")
}

fn the_subject(tenant: &Hrn) -> Hrn {
    hrn_of(tenant, "Artifact", "app")
}

/// El mundo sobre la autoridad del grafo: grafo, frontera (la unica
/// puerta de los deltas, HSP-199), evidencia, contenido y los dos
/// lectores sobre la MISMA historia.
struct World {
    graph: Arc<DgraphGraphStore>,
    frontier: Arc<DgraphProjectionFrontier>,
    deltas: Arc<DgraphDeltaLedger>,
    evidence: Arc<DgraphEvidenceStore>,
    content: Arc<InMemoryContentStore>,
    tenant: Hrn,
}

impl World {
    async fn open(client: Arc<GraphClient>, tenant: Hrn) -> Self {
        let namespace = format!("ns-{}", tenant.account_id());
        let graph = Arc::new(DgraphGraphStore::new(client.clone()));
        let frontier = Arc::new(
            DgraphProjectionFrontier::open(client.clone(), &namespace)
                .await
                .expect("the frontier opens"),
        );
        let deltas = Arc::new(DgraphDeltaLedger::new(frontier.clone()));
        let evidence = Arc::new(
            DgraphEvidenceStore::open(client)
                .await
                .expect("the evidence store opens"),
        );
        Self {
            graph,
            frontier,
            deltas,
            evidence,
            content: Arc::new(InMemoryContentStore::new()),
            tenant,
        }
    }

    /// Admite una evidencia y su contenido en el CAS.
    async fn admit(&self, id: &str, subject: &Hrn, text: &str) {
        self.content
            .put(text.as_bytes().to_vec())
            .await
            .expect("the content is stored");
        let record = evidence_record(&self.tenant, id, subject, text);
        self.evidence
            .put(record)
            .await
            .expect("the evidence is admitted");
    }

    /// Proyecta un changeset por la frontera: mutacion, deltas y
    /// checkpoint como UN solo hecho.
    async fn project(&self, changes: GraphChangeSet, position: u64) {
        let delivery = ProjectionDelivery {
            consumer: "hsp206-it".to_string(),
            stream_id: format!("stream-{}", self.tenant.account_id()),
            position,
            event_id: format!("evt-{position}-{}", nonce()),
        };
        let commit = self
            .frontier
            .apply_and_checkpoint_with(&changes, &delivery, true)
            .await
            .expect("the frontier commits");
        assert!(!commit.replayed, "a fresh delivery is never a replay");
    }

    /// El recuperador de la investigacion sobre la autoridad del grafo.
    fn retriever(&self) -> DgraphEvidenceRetriever {
        DgraphEvidenceRetriever::new(
            self.evidence.clone(),
            self.content.clone(),
            self.graph.clone(),
            self.deltas.clone(),
            Arc::new(EveryoneReads),
            self.tenant.clone(),
        )
    }

    async fn read(&self, scope: RetrievalScope) -> RetrievedContext {
        self.retriever()
            .retrieve(&scope)
            .await
            .expect("the read answers")
    }

    fn scope(&self) -> RetrievalScope {
        RetrievalScope::new(the_actor(&self.tenant), the_subject(&self.tenant))
    }

    /// El comprobador de frescura sobre la MISMA historia contra la que
    /// el recuperador sello: el oraculo de HSP-89 en la autoridad.
    fn checker(&self) -> LedgerFreshness {
        LedgerFreshness::new(
            self.evidence.clone(),
            self.deltas.clone(),
            self.tenant.clone(),
        )
    }
}

/// Los sellos de relacion de un manifiesto, ordenados.
fn relation_stamps(report: &ReadSet) -> Vec<ReadStamp> {
    let mut stamps: Vec<ReadStamp> = report
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == ReadKind::Relation)
        .cloned()
        .collect();
    stamps.sort();
    stamps
}

/// AC #1: el read-set se reproduce, el retiro de apoyo se dice Stale y
/// el veredicto es el mismo sobre los dos motores.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_read_set_is_reproduced_and_the_retirement_is_stale() {
    let Some(client) = connect().await else {
        return;
    };
    let tenant = fresh_tenant("206a");
    let world = World::open(client, tenant.clone()).await;
    let subject = the_subject(&tenant);
    let component = hrn_of(&tenant, "Component", "lib");
    let identity = format!("contains|{subject}|{component}");
    // La clave de la evidencia en la autoridad es GLOBAL (el id, sin
    // cuenta): en un alpha compartido entre ejecuciones, cada corrida
    // admite sus propias evidencias con sufijo de corrida. La referencia
    // usa los mismos ids porque es fresca en memoria.
    let run = &nonce()[7..];
    let sbom = format!("ev-{run}-sbom");
    let advisory = format!("ev-{run}-advisory");

    // El material: dos evidencias con su contenido, y una relacion
    // sostenida por la primera. Los nodos de evidencia van en el mismo
    // changeset: la relacion reclama su evidencia y el grafo no la
    // acepta sin ella.
    world.admit(&sbom, &subject, "the sbom declares lib").await;
    world
        .admit(&advisory, &subject, "the advisory warns about lib")
        .await;
    world
        .project(
            GraphChangeSet::empty()
                .with_cause(format!("assert-{}", nonce()))
                .with_node(node(&tenant, NodeKind::Artifact, "app"))
                .with_node(node(&tenant, NodeKind::Component, "lib"))
                .with_node(node(&tenant, NodeKind::Evidence, &sbom))
                .with_node(node(&tenant, NodeKind::Evidence, &advisory))
                .with_relation(relation(
                    &tenant,
                    RelationType::Contains,
                    &subject,
                    &component,
                    &sbom,
                )),
            0,
        )
        .await;

    // LECTURA SELLADA: la watermark es la version de la historia, y el
    // manifiesto dice la relacion con el apoyo que la sostiene.
    let report = world.read(world.scope()).await;
    assert_eq!(
        relation_stamps(&report.read_set),
        vec![ReadStamp::relation(&identity, &sbom)],
        "the report seals the relation with the support it read"
    );
    assert_eq!(report.coverage, Coverage::Complete, "nothing was left out");
    let sealed = report
        .read_set
        .generations
        .iter()
        .find(|(source, _)| source == GRAPH_SOURCE)
        .expect("the report says which version of the world it read")
        .1;
    assert_eq!(
        sealed, 1,
        "the Added delta of the delivery is the watermark"
    );

    // REPRODUCIBILIDAD: otra lectura a la misma watermark reproduce el
    // read-set byte a byte — el manifiesto es funcion del estado y de
    // las claves estables, nunca del orden en que el almacen respondio.
    let replay = world.read(world.scope()).await;
    assert_eq!(
        report.read_set.manifest(),
        replay.read_set.manifest(),
        "two reads at the same watermark give the same manifest"
    );
    assert_eq!(
        report.read_set.fingerprint(),
        replay.read_set.fingerprint(),
        "the seal is the byte-level identity of the read-set"
    );

    // CURRENT: el mundo no se movio bajo el informe.
    let verdict = world
        .checker()
        .freshness_of(&report.read_set, &subject)
        .await;
    assert!(
        matches!(verdict, Freshness::Current),
        "an untouched world keeps the report current: {verdict:?}"
    );

    // UN APOYO NUEVO NO CADUCA LO DICHO: la relacion gano el advisory
    // (delta SupportChanged); el informe que se apoyaba en el SBOM sigue
    // en pie, porque lo que el informe NO leyo no puede caducar lo que
    // leyo. La lectura de ahora sella el ultimo apoyo dicho — como la
    // fila de la referencia.
    world
        .project(
            GraphChangeSet::empty()
                .with_cause(format!("second-{}", nonce()))
                .with_relation(relation(
                    &tenant,
                    RelationType::Contains,
                    &subject,
                    &component,
                    &advisory,
                )),
            1,
        )
        .await;
    let verdict = world
        .checker()
        .freshness_of(&report.read_set, &subject)
        .await;
    assert!(
        matches!(verdict, Freshness::Current),
        "a support ADDED to a read relation does not retire what the report said: {verdict:?}"
    );
    let second = world.read(world.scope()).await;
    assert_eq!(
        relation_stamps(&second.read_set),
        vec![ReadStamp::relation(&identity, &advisory)],
        "the record says which support asserted it last, like the reference row"
    );

    // RETIRO DE APOYO: el advisory se retira y la relacion SIGUE
    // afirmada (le queda el SBOM). El informe sellado contra el advisory
    // es Stale con la cita de lo que se movio; el sellado contra el SBOM
    // sigue Current — el mismo veredicto de tres con el mismo material.
    world
        .project(
            GraphChangeSet::empty()
                .with_cause(format!("retire-{}", nonce()))
                .with_withdrawal(withdrawal(
                    &tenant,
                    RelationType::Contains,
                    &subject,
                    &component,
                    &advisory,
                )),
            2,
        )
        .await;
    let verdict = world
        .checker()
        .freshness_of(&second.read_set, &subject)
        .await;
    match verdict {
        Freshness::Stale { changed } => {
            assert!(
                changed.iter().any(|change| change.evidence == advisory
                    && change.resource == identity
                    && change.what == ChangeKind::Removed),
                "the verdict cites the support that moved: {changed:?}"
            );
        }
        other => panic!("a report that rests on a retired support is Stale, not {other:?}"),
    }
    let verdict = world
        .checker()
        .freshness_of(&report.read_set, &subject)
        .await;
    assert!(
        matches!(verdict, Freshness::Current),
        "the support the report read was not the one retired: {verdict:?}"
    );

    // LA RELACION SE VA CON SU ULTIMO APOYO: sin apoyos no es una
    // afirmacion (status unknown), y sale del read-set; el informe que
    // se apoyo en ella es Stale, nunca un Current fabricado de la
    // ausencia.
    world
        .project(
            GraphChangeSet::empty()
                .with_cause(format!("last-{}", nonce()))
                .with_withdrawal(withdrawal(
                    &tenant,
                    RelationType::Contains,
                    &subject,
                    &component,
                    &sbom,
                )),
            3,
        )
        .await;
    let emptied = world.read(world.scope()).await;
    assert!(
        relation_stamps(&emptied.read_set).is_empty(),
        "a relation without supports is UNKNOWN and answers no graph fact"
    );
    let verdict = world
        .checker()
        .freshness_of(&second.read_set, &subject)
        .await;
    assert!(
        matches!(verdict, Freshness::Stale { .. }),
        "the report's own support was retired under it: {verdict:?}"
    );

    // SCOPE: la cuenta ajena no se consulta; el veredicto es UNKNOWN
    // con la razon, no una respuesta sobre lo que no es suyo.
    let foreign_subject = hrn_of(&fresh_tenant("206f"), "Artifact", "other");
    let verdict = world
        .checker()
        .freshness_of(&report.read_set, &foreign_subject)
        .await;
    match verdict {
        Freshness::Unknown { reason } => {
            assert!(
                reason.contains("tenant"),
                "the reason says the subject is not of this tenant: {reason}"
            );
        }
        other => panic!("a subject of another account is UNKNOWN, not {other:?}"),
    }
}

/// AC #2: lo incompleto se DECLARA y lo ilegible es UNKNOWN — nunca un
/// hecho comprobado que no es.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_incomplete_read_is_declared_and_an_unreadable_world_is_unknown() {
    let Some(client) = connect().await else {
        return;
    };
    let tenant = fresh_tenant("206b");
    let world = World::open(client, tenant.clone()).await;
    let subject = the_subject(&tenant);

    // Tres componentes bajo tres apoyos: el presupuesto de relaciones
    // deja fuera dos. El registro de evidencia NO se admide: el read-set
    // que se corta es el de las relaciones, y la prueba no necesita mas
    // material que el grafo.
    let mut changes = GraphChangeSet::empty()
        .with_cause(format!("fanout-{}", nonce()))
        .with_node(node(&tenant, NodeKind::Artifact, "app"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-1"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-2"))
        .with_node(node(&tenant, NodeKind::Evidence, "ev-3"));
    for index in 1..=3 {
        changes = changes
            .with_node(node(&tenant, NodeKind::Component, &format!("c{index}")))
            .with_relation(relation(
                &tenant,
                RelationType::Contains,
                &subject,
                &hrn_of(&tenant, "Component", &format!("c{index}")),
                &format!("ev-{index}"),
            ));
    }
    world.project(changes, 0).await;

    let mut scope = world.scope();
    scope.budget = RetrievalBudget {
        max_relations: 1,
        ..RetrievalBudget::default()
    };
    let cut = world.read(scope).await;
    assert_eq!(
        cut.coverage,
        Coverage::Partial { omitted: 2 },
        "the cut is declared, never hidden"
    );
    assert_eq!(
        relation_stamps(&cut.read_set).len(),
        3,
        "the manifest seals what was read even when the delivery was cut"
    );
    assert_eq!(
        cut.facts
            .iter()
            .filter(|fact| fact.status == FactStatus::OmittedByBudget)
            .count(),
        2,
        "the cut facts carry the omitted status"
    );
    assert_eq!(
        cut.facts
            .iter()
            .filter(|fact| fact.status == FactStatus::Loaded)
            .count(),
        1,
        "exactly the admitted one is a fact"
    );

    // UNA AUTORIDAD MUDA CIERRA LA LECTURA: la watermark que falta no se
    // inventa con cero. El recuperador no entrega NADA — ni hechos ni
    // manifiesto — que pueda presentarse como un hecho comprobado.
    let retriever = InvestigationRetriever::new(
        world.evidence.clone(),
        world.content.clone(),
        SilentSource,
        Arc::new(EveryoneReads),
        tenant.clone(),
    );
    let failure = retriever
        .retrieve(&world.scope())
        .await
        .expect_err("a silent authority closes the read");
    assert!(
        matches!(failure, RetrievalError::Unavailable(_)),
        "the silence is an unavailable world, not an empty one: {failure:?}"
    );

    // UN MUNDO ILEGIBLE NO ES UN HECHO: un manifiesto que cita un tipo
    // de relacion que no se puede interpretar hace que la lectura de los
    // cambios falle, y el veredicto es UNKNOWN con la fuente nombrada —
    // jamas un Current inventado. La watermark se pone DETRAS de la
    // historia: es la unica puerta por la que el comprobador tiene que
    // interpretar los tipos del manifiesto, y la interpretacion falla.
    let mut forged = ReadSet::empty();
    forged.generation(GRAPH_SOURCE, 0);
    forged.read(ReadStamp::relation(
        "made-up|hrn:hodei:artifacts:eu-west-1:acme:Artifact/x|hrn:hodei:artifacts:eu-west-1:acme:Artifact/y",
        "ev-1",
    ));
    let verdict = world.checker().freshness_of(&forged, &subject).await;
    match verdict {
        Freshness::Unknown { reason } => {
            assert!(
                reason.contains(GRAPH_SOURCE),
                "the reason names the source that could not be consulted: {reason}"
            );
        }
        other => panic!("an unreadable world is UNKNOWN, not {other:?}"),
    }

    // LA HISTORIA QUE NO CONTESTA TAMPOCO ES UN HECHO: un lector de
    // deltas mudo no puede decir si el informe sigue vigente, y lo dice.
    let deaf = LedgerFreshness::new(
        world.evidence.clone(),
        Arc::new(SilentLedger),
        tenant.clone(),
    );
    let verdict = deaf.freshness_of(&cut.read_set, &subject).await;
    assert!(
        matches!(verdict, Freshness::Unknown { .. }),
        "a silent story is UNKNOWN, never Current: {verdict:?}"
    );
}
