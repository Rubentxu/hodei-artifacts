//! El journal durable de las sesiones de subida (HSP-201).
//!
//! El mapa de [`super::uploads::UploadRegistry`] es el cache local de lo
//! que aqui es la autoridad: la sesion de una subida vive **entre
//! peticiones**, y en un despliegue con dos réplicas una peticion cae en
//! cualquiera de las dos. Lo que tiene que sobrevivir al proceso es
//! minimo y cerrado: el identificador, la cuenta y el repositorio a los
//! que pertenece, el offset de bytes que lleva escritos, el handle del
//! staging del almacén, la version de revision para el CAS y la fecha de
//! su ultimo contacto para el TTL. Lo que **no** se guarda aqui son los
//! bytes: esos viven en el CAS, y el handle es la unica manera de volver
//! a ellos.
//!
//! El CAS de offset es lo que hace segura la reanudacion: cada avance
//! exige la revision que el escritor leyó y deja un **marcador de
//! revision** bajo la clave `@upsert` `{clave}#v{revision}`. Dos
//! escritores concurrentes pelean por el mismo marcador; el servidor
//! arbitra, el perdedor no escribe nada, relee el offset real y el
//! protocolo responde el `416` que dice por donde seguir. Con eso, el
//! replay de un trozo que ya entro no duplica parte alguna.
//!
//! Las implementaciones:
//!
//! - [`MemoryJournal`] —la autoridad compartida entre dos registros del
//!   mismo proceso: el seam con el que el protocolo de dos réplicas se
//!   prueba sin servidor.
//! - [`DgraphJournal`] —la autoridad sobre el grafo, con esquema
//!   **aditivo** propio (`hodei.upload.session.*`) y el mismo CAS que el
//!   libro mayor de cuota (HSP-198) usa: el marcador de revision es la
//!   condicion, no un aviso del cliente.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use hodei_graph_client::schema::dql_string;
use hodei_graph_client::{
    GraphClient, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction,
};
use serde_json::Value;

/// Por que el journal no pudo hacer lo que se le pedia.
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    /// La sesion no existe, esta cerrada o el avance choco con un
    /// cierre: en los tres casos la respuesta del protocolo es la
    /// misma —una sesion que no esta—.
    #[error("the upload session '{id}' does not exist")]
    NoSuchSession {
        /// El identificador pedido.
        id: String,
    },
    /// Otro avance gano el CAS. El registro relee el offset real y el
    /// protocolo responde el `416` con el `Range` que dice por donde
    /// seguir.
    #[error("the upload session '{id}' moved: it is at offset {offset} revision {revision}")]
    Conflict {
        /// El identificador de la sesion.
        id: String,
        /// El offset actual segun la autoridad.
        offset: u64,
        /// La revision actual segun la autoridad.
        revision: u64,
    },
    /// El journal no respondio.
    #[error("the session journal is not available: {0}")]
    Unavailable(String),
}

/// Lo que el journal recuerda de una sesion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    /// El identificador opaco de la sesion.
    pub id: String,
    /// La cuenta que la abrio: una sesion de otra cuenta no existe.
    pub account: String,
    /// El repositorio al que sube.
    pub repository: String,
    /// Los bytes que lleva escritos —el offset de la subida—.
    pub offset: u64,
    /// El handle del staging del almacén; vacio si el almacén no
    /// reanuda escrituras.
    pub handle: String,
    /// La revision del journal para el CAS: cada avance la sube de una
    /// en una y el escritor asume la que leyó.
    pub version: u64,
    /// Cerrada: commiteada, abortada o caducada. Una sesion cerrada no
    /// se reanuda.
    pub closed: bool,
    /// El ultimo contacto en millis desde el epoch: el TTL durable se
    /// cuenta desde aqui.
    pub touched_at_millis: u64,
}

/// El journal de sesiones: la autoridad que sobrevive al proceso.
#[async_trait]
pub trait SessionJournal: Send + Sync {
    /// Registra una sesion abierta.
    async fn open(&self, record: &SessionRecord) -> Result<(), JournalError>;

    /// La sesion por su identificador, abierta o cerrada.
    async fn get(&self, id: &str) -> Result<Option<SessionRecord>, JournalError>;

    /// Avanza el offset **con CAS**: `expected_version` es la revision
    /// que el escritor leyó; si la autoridad esta en otra, el avance no
    /// escribe nada y devuelve [`JournalError::Conflict`] con el offset
    /// real. El éxito sube la revision de una en una y refresca el TTL
    /// con `touched_at_millis`.
    async fn advance(
        &self,
        id: &str,
        expected_version: u64,
        offset: u64,
        touched_at_millis: u64,
    ) -> Result<u64, JournalError>;

    /// Cierra una sesion. Idempotente: cerrar algo ya cerrado es un
    /// no-op verdadero —la publicacion y el barrido compiten, y el que
    /// llega primero dice—.
    async fn close(&self, id: &str) -> Result<(), JournalError>;

    /// El barrido del TTL: las sesiones abiertas cuyo ultimo contacto
    /// pasa de la gracia se marcan cerradas y se devuelven **con su
    /// registro** para que el dueño del journal aborte el staging que
    /// dejaron. Sin esto, un crash deja sesiones abiertas y subidas
    /// multiples huerfanas para siempre.
    async fn sweep_expired(
        &self,
        now_millis: u64,
        ttl_millis: u64,
    ) -> Result<Vec<SessionRecord>, JournalError>;
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// La autoridad en memoria, compartida entre dos registros del mismo
/// proceso: el seam con el que el protocolo de dos réplicas se prueba
/// sin servidor. El CAS es un bloqueo sobre la tabla; el de Dgraph es
/// el servidor.
#[derive(Default)]
pub struct MemoryJournal {
    records: std::sync::Mutex<HashMap<String, SessionRecord>>,
}

impl MemoryJournal {
    /// Un journal vacio.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, SessionRecord>> {
        self.records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[async_trait]
impl SessionJournal for MemoryJournal {
    async fn open(&self, record: &SessionRecord) -> Result<(), JournalError> {
        let mut records = self.lock();
        if records.contains_key(&record.id) {
            return Err(JournalError::Unavailable(format!(
                "the session '{}' already stands in the journal",
                record.id
            )));
        }
        records.insert(record.id.clone(), record.clone());
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<SessionRecord>, JournalError> {
        Ok(self.lock().get(id).cloned())
    }

    async fn advance(
        &self,
        id: &str,
        expected_version: u64,
        offset: u64,
        touched_at_millis: u64,
    ) -> Result<u64, JournalError> {
        let mut records = self.lock();
        let Some(record) = records.get_mut(id) else {
            return Err(JournalError::NoSuchSession { id: id.to_owned() });
        };
        if record.closed {
            return Err(JournalError::NoSuchSession { id: id.to_owned() });
        }
        if record.version != expected_version {
            return Err(JournalError::Conflict {
                id: id.to_owned(),
                offset: record.offset,
                revision: record.version,
            });
        }
        record.version += 1;
        record.offset = offset;
        record.touched_at_millis = touched_at_millis;
        Ok(record.version)
    }

    async fn close(&self, id: &str) -> Result<(), JournalError> {
        if let Some(record) = self.lock().get_mut(id) {
            record.closed = true;
        }
        Ok(())
    }

    async fn sweep_expired(
        &self,
        now: u64,
        ttl_millis: u64,
    ) -> Result<Vec<SessionRecord>, JournalError> {
        let mut records = self.lock();
        let expired: Vec<SessionRecord> = records
            .values()
            .filter(|record| {
                !record.closed && now.saturating_sub(record.touched_at_millis) >= ttl_millis
            })
            .cloned()
            .collect();
        for record in &expired {
            if let Some(entry) = records.get_mut(&record.id) {
                entry.closed = true;
            }
        }
        Ok(expired)
    }
}

/// El tipo aditivo del journal.
pub const SESSION_TYPE: &str = "HodeiUploadSession";

/// La DDL **aditiva** del journal: predicates propios, un indice
/// `@upsert` sobre la clave —que es lo que serializa las mutaciones de
/// una sesion y hace que la revalidacion del filtro dentro de cada
/// transaccion sea el CAS—, y un tipo nuevo. No redeclara nada que otro
/// slice sea dueño.
pub const SESSION_SCHEMA_DQL: &str = "
hodei.upload.session.key: string @index(exact) @upsert .
hodei.upload.session.account: string @index(exact) .
hodei.upload.session.repository: string .
hodei.upload.session.offset: int .
hodei.upload.session.handle: string .
hodei.upload.session.version: int .
hodei.upload.session.closed: bool .
hodei.upload.session.touched_at: int .
type HodeiUploadSession {
  hodei.upload.session.key
  hodei.upload.session.account
  hodei.upload.session.repository
  hodei.upload.session.offset
  hodei.upload.session.handle
  hodei.upload.session.version
  hodei.upload.session.closed
  hodei.upload.session.touched_at
}
";

/// La autoridad sobre el grafo: el mismo CAS del libro mayor de cuota,
/// arbitrado por el servidor.
pub struct DgraphJournal {
    client: Arc<GraphClient>,
    tenant: String,
}

impl DgraphJournal {
    /// Abre el journal sobre el grafo, aplicando su DDL aditiva.
    pub async fn open(client: Arc<GraphClient>, tenant: &str) -> Result<Self, JournalError> {
        if tenant.is_empty()
            || !tenant
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(JournalError::Unavailable(format!(
                "the journal tenant '{tenant}' is not a safe key part"
            )));
        }
        apply_session_schema(&client).await?;
        Ok(Self {
            client,
            tenant: tenant.to_owned(),
        })
    }

    /// La clave de una sesion. El tenant esta validado y el id es un
    /// UUID: ninguna de las dos partes introduce el separador, y la
    /// clave es estable para dos escritores del mismo id.
    fn key_of(&self, id: &str) -> String {
        format!("upload-session/{}/{}", self.tenant, id)
    }

    /// La clave del marcador de una revision: el ancla de conflicto del
    /// CAS, creada solo si falta —que es lo que fuerza a dos escritores
    /// de la misma revision a pelear por la misma clave `@upsert`.
    fn marker_key(&self, id: &str, revision: u64) -> String {
        format!("{}#v{revision}", self.key_of(id))
    }

    async fn read(&self, id: &str) -> Result<Option<SessionRecord>, JournalError> {
        let query = format!(
            "{{ q(func: eq(hodei.upload.session.key, {key})) {{ \
             hodei.upload.session.account hodei.upload.session.repository \
             hodei.upload.session.offset hodei.upload.session.handle \
             hodei.upload.session.version hodei.upload.session.closed \
             hodei.upload.session.touched_at }} }}",
            key = dql_string(&self.key_of(id)),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| JournalError::Unavailable(error.to_string()))?;
        require_no_errors(&response.json)?;
        let nodes = response
            .json
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let Some(node) = nodes.first() else {
            return Ok(None);
        };
        Ok(Some(SessionRecord {
            id: id.to_owned(),
            account: text_of(node, "hodei.upload.session.account").unwrap_or_default(),
            repository: text_of(node, "hodei.upload.session.repository").unwrap_or_default(),
            offset: node
                .get("hodei.upload.session.offset")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            handle: text_of(node, "hodei.upload.session.handle").unwrap_or_default(),
            version: node
                .get("hodei.upload.session.version")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            closed: node
                .get("hodei.upload.session.closed")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            touched_at_millis: node
                .get("hodei.upload.session.touched_at")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        }))
    }
}

#[async_trait]
impl SessionJournal for DgraphJournal {
    async fn open(&self, record: &SessionRecord) -> Result<(), JournalError> {
        let key = self.key_of(&record.id);
        let nquads = format!(
            "_:s <dgraph.type> \"{session_type}\" .\n\
             _:s <hodei.upload.session.key> {key} .\n\
             _:s <hodei.upload.session.account> {account} .\n\
             _:s <hodei.upload.session.repository> {repository} .\n\
             _:s <hodei.upload.session.offset> \"{offset}\" .\n\
             _:s <hodei.upload.session.handle> {handle} .\n\
             _:s <hodei.upload.session.version> \"{version}\" .\n\
             _:s <hodei.upload.session.closed> \"false\" .\n\
             _:s <hodei.upload.session.touched_at> \"{touched}\" .\n",
            session_type = SESSION_TYPE,
            key = dql_string(&key),
            account = dql_string(&record.account),
            repository = dql_string(&record.repository),
            offset = record.offset,
            handle = dql_string(&record.handle),
            version = record.version,
            touched = record.touched_at_millis,
        );
        let response = self
            .client
            .upsert(GraphUpsertTransaction {
                query: format!(
                    "{{ q as var(func: eq(hodei.upload.session.key, {key})) }}",
                    key = dql_string(&key),
                ),
                blocks: vec![GraphMutationBlock::set_if("eq(len(q), 0)", nquads)],
                commit_now: true,
            })
            .await
            .map_err(|error| JournalError::Unavailable(error.to_string()))?;
        require_no_errors(&response.json)
    }

    async fn get(&self, id: &str) -> Result<Option<SessionRecord>, JournalError> {
        self.read(id).await
    }

    async fn advance(
        &self,
        id: &str,
        expected_version: u64,
        offset: u64,
        touched_at_millis: u64,
    ) -> Result<u64, JournalError> {
        let key = self.key_of(id);
        let marker = self.marker_key(id, expected_version + 1);
        // La transaccion: la sesion revalidada EN SU PROPIA SNAPSHOT en
        // la revision que el escritor asumio y sin cerrar, y el
        // marcador de la siguiente revision AUN SIN EXISTIR. El filtro
        // se re-evalua bajo la serializacion del indice `@upsert` de la
        // clave, de modo que dos avances de la misma revision no pueden
        // casar los dos: el segundo ve la revision movida o el marcador
        // puesto, su bloque no escribe nada y pierde.
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{\n  s as var(func: eq(hodei.upload.session.key, {key})) \
                 @filter(eq(hodei.upload.session.version, {expected}) AND eq(hodei.upload.session.closed, false))\n  \
                 t as var(func: eq(hodei.upload.session.key, {marker}))\n}}",
                key = dql_string(&key),
                expected = expected_version,
                marker = dql_string(&marker),
            ),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(s), 1) AND eq(len(t), 0)",
                format!(
                    "uid(s) <hodei.upload.session.offset> \"{offset}\" .\n\
                     uid(s) <hodei.upload.session.version> \"{next}\" .\n\
                     uid(s) <hodei.upload.session.touched_at> \"{touched}\" .\n\
                     _:t <dgraph.type> \"{session_type}\" .\n\
                     _:t <hodei.upload.session.key> {marker} .\n",
                    offset = offset,
                    next = expected_version + 1,
                    touched = touched_at_millis,
                    session_type = SESSION_TYPE,
                    marker = dql_string(&marker),
                ),
            )],
            commit_now: true,
        };
        let response = self
            .client
            .upsert(transaction)
            .await
            .map_err(|error| JournalError::Unavailable(error.to_string()))?;
        require_no_errors(&response.json)?;
        let won = response
            .json
            .get("s")
            .and_then(Value::as_array)
            .map(|nodes| !nodes.is_empty())
            .unwrap_or(false);
        if won {
            return Ok(expected_version + 1);
        }
        // El filtro no casó: o la sesion no esta, o esta cerrada, u otro
        // avance gano. Se relee para decir cual.
        match self.read(id).await? {
            None => Err(JournalError::NoSuchSession { id: id.to_owned() }),
            Some(record) if record.closed => Err(JournalError::NoSuchSession { id: id.to_owned() }),
            Some(record) => Err(JournalError::Conflict {
                id: id.to_owned(),
                offset: record.offset,
                revision: record.version,
            }),
        }
    }

    async fn close(&self, id: &str) -> Result<(), JournalError> {
        let key = self.key_of(id);
        let response = self
            .client
            .upsert(GraphUpsertTransaction {
                query: format!(
                    "{{ s as var(func: eq(hodei.upload.session.key, {key})) \
                     @filter(eq(hodei.upload.session.closed, false)) }}",
                    key = dql_string(&key),
                ),
                blocks: vec![GraphMutationBlock::set_if(
                    "eq(len(s), 1)",
                    "uid(s) <hodei.upload.session.closed> \"true\" .\n",
                )],
                commit_now: true,
            })
            .await
            .map_err(|error| JournalError::Unavailable(error.to_string()))?;
        require_no_errors(&response.json)
    }

    async fn sweep_expired(
        &self,
        now: u64,
        ttl_millis: u64,
    ) -> Result<Vec<SessionRecord>, JournalError> {
        let deadline = now.saturating_sub(ttl_millis);
        let response = self
            .client
            .execute_dql(&format!(
                "{{ q(func: eq(hodei.upload.session.closed, false)) \
                 @filter(le(hodei.upload.session.touched_at, {deadline})) {{ \
                 hodei.upload.session.key hodei.upload.session.account \
                 hodei.upload.session.repository hodei.upload.session.offset \
                 hodei.upload.session.handle hodei.upload.session.version \
                 hodei.upload.session.touched_at }} }}",
                deadline = deadline,
            ))
            .await
            .map_err(|error| JournalError::Unavailable(error.to_string()))?;
        require_no_errors(&response.json)?;
        let nodes = response
            .json
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut expired = Vec::new();
        for node in nodes {
            let key = match text_of(&node, "hodei.upload.session.key") {
                Some(key) => key,
                None => continue,
            };
            let version = node
                .get("hodei.upload.session.version")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            // La marca de cierre se apoya en la version leida: si alguien
            // reanudo y avanzo la sesion entre el listado y la marca, el
            // filtro no casa, el barrido no la toca y decide la proxima
            // pasada. Cerrar no sube la version —no hay CAS que perder:
            // dos cierres del mismo estado son el mismo hecho—.
            let closed = self
                .client
                .upsert(GraphUpsertTransaction {
                    query: format!(
                        "{{ s as var(func: eq(hodei.upload.session.key, {key})) \
                         @filter(eq(hodei.upload.session.version, {version}) AND eq(hodei.upload.session.closed, false)) }}",
                        key = dql_string(&key),
                        version = version,
                    ),
                    blocks: vec![GraphMutationBlock::set_if(
                        "eq(len(s), 1)",
                        "uid(s) <hodei.upload.session.closed> \"true\" .\n",
                    )],
                    commit_now: true,
                })
                .await
                .map_err(|error| JournalError::Unavailable(error.to_string()))?;
            require_no_errors(&closed.json)?;
            let marked = closed
                .json
                .get("s")
                .and_then(Value::as_array)
                .map(|nodes| !nodes.is_empty())
                .unwrap_or(false);
            if !marked {
                continue;
            }
            let id = key.rsplit('/').next().unwrap_or(&key).to_owned();
            expired.push(SessionRecord {
                id,
                account: text_of(&node, "hodei.upload.session.account").unwrap_or_default(),
                repository: text_of(&node, "hodei.upload.session.repository").unwrap_or_default(),
                offset: node
                    .get("hodei.upload.session.offset")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                handle: text_of(&node, "hodei.upload.session.handle").unwrap_or_default(),
                version,
                closed: true,
                touched_at_millis: node
                    .get("hodei.upload.session.touched_at")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            });
        }
        Ok(expired)
    }
}

/// La DDL del journal es aditiva e idempotente; dos aperturas
/// simultaneas alteran el mismo slice y el servidor puede rechazar una
/// mientras asienta los indices: se reintenta poco y se falla tarde.
async fn apply_session_schema(client: &GraphClient) -> Result<(), JournalError> {
    let mut last = String::new();
    for attempt in 1..=3 {
        match client.alter_schema(SESSION_SCHEMA_DQL).await {
            Ok(()) => return Ok(()),
            Err(error) if attempt < 3 => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(150 * attempt as u64)).await;
            }
            Err(error) => return Err(JournalError::Unavailable(error.to_string())),
        }
    }
    Err(JournalError::Unavailable(format!(
        "the session DDL slice did not settle: {last}"
    )))
}

/// Los errores que el alambre trae dentro del JSON nunca se leen como
/// exito.
fn require_no_errors(json: &Value) -> Result<(), JournalError> {
    let empty: Vec<Value> = Vec::new();
    let errors = json
        .get("errors")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    if errors.is_empty() {
        return Ok(());
    }
    let text = errors
        .iter()
        .filter_map(|error| error.get("message").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("; ");
    Err(JournalError::Unavailable(if text.is_empty() {
        format!("{errors:?}")
    } else {
        text
    }))
}

fn text_of(node: &Value, predicate: &str) -> Option<String> {
    node.get(predicate)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// El reloj del journal: millis desde el epoch, la misma unidad que las
/// fechas del grafo.
pub fn journal_now() -> u64 {
    now_millis()
}
