//! Las sesiones de subida: el estado que vive entre peticiones.
//!
//! El protocolo de subida es **por trozos**: un `POST` abre la sesion, un
//! `PATCH` (o varios) la llenan y un `PUT` la cierra. Entre una peticion
//! y otra hay estado que hay que recordar: cuantos bytes han entrado, que
//! sesion de escritura del CAS los esta guardando, de que cuenta y de que
//! repositorio es la subida.
//!
//! **Sin journal**, ese estado vive aqui, en memoria, y por tres razones:
//!
//! - **El identificador es opaco y no se puede adivinar.** Es un UUID de
//!   128 bits, y una sesion que se pudiera adivinar la podria escribir
//!   cualquiera.
//! - **La cuenta y el repositorio son parte de la identidad de la
//!   sesion.** Una sesion de otra cuenta no es "tu sesion con otro
//!   identificador": es una sesion que no existe, y se responde como
//!   tal.
//! - **La sesion sobrevive a la peticion, no al proceso.** Si el proceso
//!   se cae, la sesion se pierde y el cliente recibe un error explicito
//!   (`BLOB_UPLOAD_UNKNOWN`), no una subida que se queda colgada ni un
//!   contenido a medias. El fichero de staging que dejo se limpia al
//!   arrancar, como cualquier staging abandonado.
//!
//! **Con journal durable** (HSP-201) el mapa pasa a ser un **cache**: la
//! autoridad del offset vive fuera del proceso, en
//! [`SessionJournal`](crate::oci::session_journal::SessionJournal), y con
//! ella:
//!
//! - El avance del offset va por **CAS de revision**: dos escritoras de
//!   una misma sesion no avanzan las dos —la que pierde recibe el offset
//!   real y el protocolo responde el `416` con `Range`, que es lo que
//!   hace que reenviar un trozo no duplique parte alguna—.
//! - La sesion se **reanima en cualquier réplica** contra el handle del
//!   staging del CAS: la ruta del fichero, o el par `key`/`upload_id`
//!   del multipart. El mango del almacén es un **recurso reclamable**,
//!   no una propiedad del proceso: cada escritura dura lo que dura una
//!   peticion y se suelta, y el siguiente trozo lo retoma aqui o en
//!   otra réplica. La exclusión física la pone el almacén —el bloqueo
//!   advisory del fichero—.
//! - Un mango que muere con su proceso **no borra** un staging que la
//!   autoridad aún tiene abierto: su destino lo deciden el journal, el
//!   commit, el abort explicito y el barrido ([`UploadRegistry::sweep`]).
//!
//! Lo que **no** se guarda aqui es el contenido: los bytes van al CAS por
//! su sesion de escritura, y el staging de cada subida es un fichero
//! propio, para que dos subidas del mismo contenido no se pisen.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hodei_hub::blob::BlobWriteSession;

use crate::oci::failure::OciFailure;
use crate::oci::session_journal::{SessionJournal, SessionRecord, journal_now};

/// Una sesion de subida en curso.
///
/// El escritor es la sesion de escritura del almacen de contenido: los
/// bytes van a su staging, y el commit es lo unico que publica. Con
/// journal puede estar **soltado** —la escritura de esta peticion
/// termino y el mango volvio al almacén—; reanimarlo es tarea de
/// [`UploadRegistry::find_or_resume`].
pub struct UploadSession {
    /// El identificador opaco de la sesion.
    id: String,
    /// La escritura en curso del almacen de contenido, si este proceso
    /// la tiene tomada ahora.
    writer: Option<Box<dyn BlobWriteSession>>,
    /// La reserva de la cuota que admitio esta subida.
    ///
    /// Vive **aqui** y no en la peticion porque la subida vive aqui y no
    /// en la peticion: la reserva de una sesion abandonada o caducada
    /// vuelve al libro mayor cuando la sesion muere, y lo que una sesion
    /// commitea se asienta al cerrarse.
    ticket: crate::quota::QuotaTicket,
    /// Cuando se toco por ultima vez.
    last_seen: Instant,
    /// La revision del journal que este registro leyo: el CAS del offset
    /// avanza **desde aqui** y ninguna escritura a ciegas mueve la
    /// autoridad. Cero cuando el registro no tiene journal durable.
    journal_version: u64,
    /// Si el journal guardo esta sesion: una sesion journalizada se
    /// reanuda en otra réplica y su staging es durable.
    journaled: bool,
    /// El handle del staging que la autoridad guardo: vacio cuando el
    /// almacén no reanuda.
    handle: String,
}

impl UploadSession {
    /// El identificador de la sesion.
    pub fn the_id(&self) -> &str {
        &self.id
    }

    /// Los bytes que han entrado, según el mango local.
    ///
    /// `None` cuando el mango esta soltado: este proceso no conoce el
    /// offset sin preguntar a la autoridad.
    pub fn the_local_offset(&self) -> Option<u64> {
        self.writer.as_ref().map(|writer| writer.bytes_written())
    }

    /// La revision del journal que el registro tiene en la mano.
    pub fn the_journal_version(&self) -> u64 {
        self.journal_version
    }

    /// Anota la revision que el journal admitio tras un avance.
    pub fn set_journal_version(&mut self, version: u64) {
        self.journal_version = version;
    }

    /// El handle del staging que la autoridad guardo para esta sesion.
    pub fn the_handle(&self) -> &str {
        &self.handle
    }

    /// Si el journal guardo esta sesion: una sesion journalizada se
    /// reanuda en otra réplica y su staging es durable.
    pub fn is_journaled(&self) -> bool {
        self.journaled
    }

    /// El escritor de la sesion, para escribir y para cerrar.
    ///
    /// `None` cuando el mango esta soltado: la peticion que necesita
    /// escribir vuelve por [`UploadRegistry::find_or_resume`], que lo
    /// reanima.
    pub fn the_writer(&mut self) -> Option<&mut Box<dyn BlobWriteSession>> {
        self.writer.as_mut()
    }

    /// El escritor y la reserva, juntos y prestados sin pelearse.
    ///
    /// Escribir un trozo necesita tomar los dos a la vez: el escritor
    /// para el byte y la cuota para pincharlo **antes** de escribirlo.
    /// Son campos distintos, y la toma prestada lo dice en el tipo.
    pub fn the_writer_and_ticket(
        &mut self,
    ) -> Option<(
        &mut Box<dyn BlobWriteSession>,
        &mut crate::quota::QuotaTicket,
    )> {
        let writer = self.writer.as_mut()?;
        Some((writer, &mut self.ticket))
    }

    /// Asienta la reserva contra lo que el almacén ganó al commitear.
    pub fn settle(&mut self, stored_new_bytes: u64) {
        self.ticket.settle(stored_new_bytes);
    }

    /// Anota que la sesion se ha tocado.
    ///
    /// La caducidad se cuenta desde el ultimo uso, no desde el
    /// principio: una subida larga vive mientras alguien la escribe, y
    /// una sesion que nadie toca es una sesion que hay que olvidar.
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
    }

    /// Deja en el almacén lo que ya es durable y devuelve ese offset.
    ///
    /// Es el offset que el journal puede admitir: en el almacen de
    /// ficheros es todo lo escrito —el fichero sincronizado—; en el
    /// multipart son los trozos confirmados por el servidor, con lo
    /// en vuelo ya recogido. Lo que queda en el buffer muere con el
    /// proceso y el cliente lo reenvia contra el `Range` que la
    /// autoridad declara.
    pub async fn flush_durable(&mut self) -> Result<u64, hodei_hub::blob::BlobError> {
        match self.writer.as_mut() {
            Some(writer) => writer.flush_durable().await,
            // Un mango soltado no tiene nada nuevo que dejar: la
            // autoridad ya declaro lo que dejo.
            None => Ok(0),
        }
    }

    /// El mango que la autoridad tiene soltado: no hay nada local que
    /// declarar.
    pub fn the_durable_offset(&self) -> Option<u64> {
        self.writer
            .as_ref()
            .map(|writer| writer.the_durable_offset())
    }

    /// Si el digest del mango cubre el staging entero.
    ///
    /// Un mango reanimado en frio no paso por los bytes que ya estaban
    /// en el almacén: quien cierra no compara ese digest parcial contra
    /// lo declarado, y deja que el almacén compruebe el contenido
    /// completo al publicar —la misma verificacion honesta, una vez—.
    pub fn the_digest_covers_all(&self) -> bool {
        match self.writer.as_ref() {
            Some(writer) => writer.the_digest_covers_all(),
            None => false,
        }
    }

    /// Suelta el mango local cuando la sesion journalizada ya no lo
    /// necesita.
    ///
    /// Con journal y handle, el staging del almacén es durable: cerrar
    /// el fichero —liberando su bloqueo advisory— **no** borra nada, y
    /// deja el mango libre para que la proxima peticion lo reclame aqui
    /// o en otra réplica. Es la mano que cambia de proceso, y es lo que
    /// hace que el `PATCH` de A y el `PATCH` de B existan a la vez.
    /// Solo se suelta cuando no queda nada por debajo de lo durable: el
    /// multipart no puede soltar un buffer que aun debe bytes al
    /// almacén, y soltarse abortaria sus trozos —su mango se queda
    /// hasta el commit, el abort o el barrido—.
    pub fn release_writer_when_possible(&mut self) {
        let Some(writer) = self.writer.as_mut() else {
            return;
        };
        if !self.journaled || self.handle.is_empty() {
            return;
        }
        if writer.the_durable_offset() != writer.bytes_written() {
            return;
        }
        if writer.detach().is_ok() {
            self.writer = None;
        }
    }

    /// Anota el handle con el que la autoridad reabrio la sesion.
    fn set_handle(&mut self, handle: &str) {
        self.handle = handle.to_string();
    }
}

/// El resultado de avanzar el offset de una sesion journalizada.
#[derive(Debug)]
pub enum Advance {
    /// La autoridad admitio el avance: la nueva revision.
    Admitted(u64),
    /// Otro escritor gano el CAS: el offset que la autoridad tiene. El
    /// protocolo responde el `416` con el `Range` de seguir —con esto,
    /// el replay de un trozo ya escrito no duplica parte alguna—.
    Conflict(u64),
    /// La sesion ya no existe para la autoridad (cerrada o barrida).
    Gone,
    /// La autoridad no respondio.
    Unavailable(String),
}

/// El registro de las sesiones de subida.
///
/// El mapa esta bajo un mutex **sync** porque sus operaciones son cortas
/// y no esperan: lo que espera es la sesion, y cada sesion tiene su
/// propio mutex asincrono. Dos subidas a la vez no se bloquean entre si.
///
/// Con journal durable, el mapa pasa a ser un **cache**: la autoridad
/// del offset es el journal, y una sesion que este proceso no conoce se
/// reanuda desde él ([`UploadRegistry::find_or_resume`]) con el CAS de
/// revision. Eso es lo que hace que el protocolo sobreviva a una réplica:
/// los bytes no viven aqui, viven en el CAS, y el handle del staging
/// basta para volver a ellos. El TTL local caduca el **asiento del
/// cache**, no la sesion: un asiento caducado suelta su mango sin borrar
/// el staging durable, y la autoridad la caduca por su cuenta con el
/// mismo TTL ([`UploadRegistry::sweep`]).
pub struct UploadRegistry {
    /// Las sesiones vivas **en este proceso**, por identificador.
    sessions: Mutex<HashMap<String, Entry>>,
    /// Cuanto vive una sesion sin que nadie la toque.
    ttl: Duration,
    /// Cuantas sesiones pueden vivir a la vez.
    max_sessions: usize,
    /// La autoridad durable del offset: sin ella, las sesiones mueren
    /// con el proceso, como siempre.
    journal: Option<Arc<dyn SessionJournal>>,
}

impl Default for UploadRegistry {
    /// Un registro con los limites de siempre.
    ///
    /// Una sesion vive una hora y hay 64 a la vez. Son los limites que
    /// un despliegue puede cambiar, y los que un despliegue que no los
    /// cambia tiene.
    fn default() -> Self {
        Self::new(Duration::from_secs(3600), 64)
    }
}

impl UploadRegistry {
    /// Un registro vacio, sin journal durable.
    ///
    /// El TTL es el tiempo que una sesion sobrevive sin que nadie la
    /// toque, y el tope es cuantas sesiones pueden existir a la vez: un
    /// registro sin tope es un registro donde un cliente abre sesiones y
    /// las abandona, y cada una deja un fichero de staging en disco.
    pub fn new(ttl: Duration, max_sessions: usize) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            ttl,
            max_sessions,
            journal: None,
        }
    }

    /// Un registro con journal durable: la autoridad del offset que
    /// sobrevive al proceso y a la réplica.
    pub fn with_journal(
        ttl: Duration,
        max_sessions: usize,
        journal: Arc<dyn SessionJournal>,
    ) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            ttl,
            max_sessions,
            journal: Some(journal),
        }
    }

    /// Abre una sesion y la registra.
    ///
    /// El identificador es un UUID: lo que el cliente recibe en el
    /// `Location` y lo que devuelve en cada peticion siguiente.
    ///
    /// La cuota ya decidio antes de llamar: el `ticket` es la reserva
    /// que la subida trae consigo, y la sesion la conserva hasta que se
    /// asienta al commitear o se suelta al morir.
    pub async fn open(
        &self,
        account: &str,
        repository: &str,
        writer: Box<dyn BlobWriteSession>,
        ticket: crate::quota::QuotaTicket,
    ) -> Result<Arc<tokio::sync::Mutex<UploadSession>>, OciFailure> {
        let id = uuid::Uuid::new_v4().hyphenated().to_string();
        let handle = writer.the_staging_handle().unwrap_or_default();
        let journaled = self.journal.is_some();
        let record = SessionRecord {
            id: id.clone(),
            account: account.to_string(),
            repository: repository.to_string(),
            offset: writer.bytes_written(),
            handle: handle.clone(),
            version: 0,
            closed: false,
            touched_at_millis: journal_now(),
        };
        let session = Arc::new(tokio::sync::Mutex::new(UploadSession {
            id: id.clone(),
            writer: Some(writer),
            ticket,
            last_seen: Instant::now(),
            journal_version: 0,
            journaled,
            handle: handle.clone(),
        }));
        // El asiento se reserva con el tope comprobado **bajo el mismo
        // golpe de llave**: dos `POST` concurrentes no pueden colarse
        // los dos por la ranura. El guard se suelta antes de cualquier
        // await: un journal durable espera, y el mapa no puede dormir
        // con la llave en la mano.
        {
            let mut sessions = self.lock();
            // Las sesiones caducadas se van antes de contar: un registro
            // que cuenta las muertas no puede saber si cabe una mas.
            let expired = the_expired(&sessions, self.ttl);
            for old_id in &expired {
                sessions.remove(old_id);
            }
            if sessions.len() >= self.max_sessions {
                return Err(OciFailure::blob_upload_invalid(format!(
                    "there are already {} uploads in progress",
                    self.max_sessions
                )));
            }
            sessions.insert(
                id.clone(),
                Entry {
                    account: account.to_string(),
                    repository: repository.to_string(),
                    session: Arc::clone(&session),
                },
            );
        }
        // La autoridad escribe despues de reservar el asiento y antes
        // de que el `Location` llegue al cliente: una sesion cuyo
        // journal fallo **no** existe —el asiento se retira y el staging
        // se limpia con el Drop del mango—, porque una sesion que existe
        // solo aqui es exactamente el estado que este slice saca del
        // proceso.
        if let Some(journal) = &self.journal {
            if let Err(error) = journal.open(&record).await {
                self.remove(&id);
                return Err(journal_unavailable(error));
            }
            let mut session = session.lock().await;
            // La autoridad tiene la sesion: el staging pasa a ser suyo.
            // Un mango local que muere —desalojo del cache, réplica que
            // cambia, caida del proceso— no borra bytes que otra réplica
            // puede reanudar; lo que decide su destino es el journal y
            // su barrido.
            if !handle.is_empty()
                && let Some(writer) = session.the_writer()
            {
                writer.mark_durable();
            }
            // Un `POST` que no escribe un solo byte puede soltar el
            // mango ya: la proxima peticion —aqui o en otra réplica— lo
            // retoma por el handle.
            session.release_writer_when_possible();
        }
        Ok(session)
    }

    /// La sesion de una subida **en cualquier réplica**.
    ///
    /// Primero el cache local; si aqui no existe, o existe con el mango
    /// soltado, la autoridad: una sesion abierta en otra réplica se
    /// reanuda reabriendo el handle del CAS, con el offset que el
    /// journal declare y el scope que el registro exijo. Una sesion de
    /// otra cuenta **no existe** —ni siquiera se reanuda: el scope se
    /// comprueba contra la autoridad antes de tocar el almacén—.
    ///
    /// `Err` solo cuando la autoridad no respondio: una sesion que el
    /// journal tiene abierta no puede convertirse en un `404` porque el
    /// journal estuviera caido —eso es 503, y el cliente reintenta—.
    pub async fn find_or_resume(
        &self,
        account: &str,
        repository: &str,
        id: &str,
        store: &dyn hodei_hub::blob::BlobStore,
    ) -> Result<Option<Arc<tokio::sync::Mutex<UploadSession>>>, OciFailure> {
        if let Some(shared) = self.find(account, repository, id) {
            let reanimate = {
                let session = shared.lock().await;
                session.writer.is_none() && session.journaled && !session.handle.is_empty()
            };
            if !reanimate {
                return Ok(Some(shared));
            }
            // El mango se solto —al abrir la sesion o en otra peticion—:
            // la autoridad dice el offset y el almacén reabre el
            // staging.
            let record = self.the_journal_record(id).await?.ok_or_else(|| {
                OciFailure::blob_upload_unknown("the upload session does not exist")
            })?;
            if record.account != account || record.repository != repository || record.closed {
                return Ok(None);
            }
            let writer = self.resume_writer(&record, store).await?;
            {
                let mut session = shared.lock().await;
                match writer {
                    Some(writer) => {
                        session.writer = Some(writer);
                        session.journal_version = record.version;
                        session.set_handle(&record.handle);
                    }
                    // Un almacén que no reanuda no puede reabrir: la
                    // sesion existe y el mango se perdio con el proceso
                    // que lo tenia. El reintento caera donde haya
                    // handle.
                    None => {
                        return Err(OciFailure::unavailable(
                            "the upload session cannot be resumed in this process",
                        ));
                    }
                }
            }
            return Ok(Some(shared));
        }
        let Some(journal) = &self.journal else {
            return Ok(None);
        };
        let record = journal.get(id).await.map_err(journal_unavailable)?;
        let Some(record) = record else {
            return Ok(None);
        };
        // El ambito es parte de la identidad: una sesion de otra cuenta
        // o de otro repositorio no existe, y la autoridad lo dice sin
        // mirar el staging.
        if record.account != account || record.repository != repository || record.closed {
            return Ok(None);
        }
        let Some(writer) = self.resume_writer(&record, store).await? else {
            // La sesion existe para la autoridad y su handle esta
            // vacio: el almacén de este despliegue no reanuda
            // escrituras. Un `404` negaria una sesion que la autoridad
            // tiene abierta; es 503 con reintento —en otra réplica con
            // un almacén que si reanuda, o cuando el barrido la
            // cierre, el resultado sera el mismo—.
            return Err(OciFailure::unavailable(
                "the upload session cannot be resumed in this process",
            ));
        };
        // La reserva local que acompana al mango: un libro mayor **sin
        // limites** —la cuota durable entre réplicas no la fija este
        // cache; la fija la reserva de cuota del registro, y aqui solo
        // vuelve a empezar a contar lo que **esta** réplica escribe—.
        let ticket = crate::quota::QuotaLedger::new(crate::quota::QuotaLimits::default())
            .begin(account, None)
            .map_err(|_| OciFailure::unavailable("the quota ledger is saturated"))?;
        let session = Arc::new(tokio::sync::Mutex::new(UploadSession {
            id: record.id.clone(),
            writer: Some(writer),
            ticket,
            last_seen: Instant::now(),
            journal_version: record.version,
            journaled: true,
            handle: record.handle.clone(),
        }));
        self.lock().insert(
            record.id.clone(),
            Entry {
                account: account.to_owned(),
                repository: repository.to_owned(),
                session: Arc::clone(&session),
            },
        );
        Ok(Some(session))
    }

    /// La via del `GET` de estado: el offset que la autoridad declara
    /// para una sesion que esta réplica no tiene escrita.
    ///
    /// Devuelve el offset **sin** reabrir el almacén: consultar cuanto
    /// lleva una subida no necesita tomar el mango ni excluir a quien
    /// lo tiene.
    pub async fn the_journal_offset(
        &self,
        account: &str,
        repository: &str,
        id: &str,
    ) -> Result<Option<u64>, OciFailure> {
        let record = self.the_journal_record(id).await?;
        Ok(match record {
            Some(record)
                if record.account == account
                    && record.repository == repository
                    && !record.closed =>
            {
                Some(record.offset)
            }
            _ => None,
        })
    }

    /// La autoridad de la sesion, si el registro tiene autoridad.
    async fn the_journal_record(&self, id: &str) -> Result<Option<SessionRecord>, OciFailure> {
        let Some(journal) = &self.journal else {
            return Ok(None);
        };
        journal.get(id).await.map_err(journal_unavailable)
    }

    /// Reabre el mango del CAS sobre el handle que la autoridad guardo.
    ///
    /// Un almacén que no reanuda (el de memoria) o un handle vacio
    /// devuelven `None`: la sesion **existe** para la autoridad y no
    /// puede escribirse desde el cache.
    async fn resume_writer(
        &self,
        record: &SessionRecord,
        store: &dyn hodei_hub::blob::BlobStore,
    ) -> Result<Option<Box<dyn BlobWriteSession>>, OciFailure> {
        if record.handle.is_empty() {
            return Ok(None);
        }
        let writer = store
            .resume(&record.handle, record.offset)
            .await
            .map_err(the_resume_error)?;
        Ok(Some(writer))
    }

    /// El avance del offset de una sesion, arbitrado por el journal.
    ///
    /// El registro llama **despues** de escribir el trozo: la autoridad
    /// solo admite el offset de los bytes que ya estan en el CAS —por
    /// eso avanza hasta `flush_durable`, no hasta lo que el buffer del
    /// mango lleva en el proceso—. Un conflicto de CAS no es un fallo
    /// —es el `416` del protocolo: el otro escritor gano, la autoridad
    /// dice por donde seguir—.
    pub async fn advance(&self, id: &str, expected_version: u64, offset: u64) -> Advance {
        let Some(journal) = &self.journal else {
            return Advance::Admitted(expected_version);
        };
        match journal
            .advance(id, expected_version, offset, journal_now())
            .await
        {
            Ok(version) => Advance::Admitted(version),
            Err(crate::oci::session_journal::JournalError::Conflict { offset, .. }) => {
                Advance::Conflict(offset)
            }
            Err(crate::oci::session_journal::JournalError::NoSuchSession { .. }) => Advance::Gone,
            Err(error) => Advance::Unavailable(error.to_string()),
        }
    }

    /// Cierra la autoridad de una sesion: commiteada o abortada.
    pub async fn close(&self, id: &str) -> Result<(), OciFailure> {
        if let Some(journal) = &self.journal {
            journal.close(id).await.map_err(journal_unavailable)?;
        }
        self.remove(id);
        Ok(())
    }

    /// El barrido durable: las sesiones que **otra réplica** dejo
    /// abiertas al caerse se marcan cerradas y su staging huerfano se
    /// aborta contra el almacén. Es la reconciliacion de AC3: sin esto,
    /// un crash deja un multipart abierto para siempre con bytes que
    /// nadie verifico. Idempotente: cerrar dos veces es cerrar una.
    pub async fn sweep(&self, store: &dyn hodei_hub::blob::BlobStore) -> Result<usize, OciFailure> {
        let Some(journal) = &self.journal else {
            return Ok(0);
        };
        let ttl_millis = self.ttl.as_millis() as u64;
        let expired = journal
            .sweep_expired(journal_now(), ttl_millis)
            .await
            .map_err(|error| {
                OciFailure::unavailable(format!("the session sweep failed: {error}"))
            })?;
        let mut reconciled = 0usize;
        for record in &expired {
            // Una sesion de este proceso con el asiento tomado no es
            // huerfana: su Drop o su abort se encargan. Solo lo que este
            // proceso no tiene se reconcilia aqui.
            if self.lock().contains_key(&record.id) {
                continue;
            }
            if record.handle.is_empty() {
                // El almacén de este staging no reanuda: su staging
                // murio con el proceso; la autoridad ya la cerro.
                reconciled += 1;
                continue;
            }
            // Reabrir para abortar: un staging vivo en otra réplica no
            // se puede tomar (el bloqueo advisory lo impide), y uno que
            // ya no esta tampoco. Lo que se toma se cierra **sin
            // publicar**: abort, nunca commit.
            if let Ok(mut writer) = store.resume(&record.handle, record.offset).await {
                let _ = writer.abort().await;
                reconciled += 1;
            }
        }
        Ok(reconciled)
    }

    /// La sesion de una subida, si es de esta cuenta y este repositorio.
    ///
    /// Una sesion de otra cuenta **no existe**: el mismo error que una
    /// sesion que no esta. Un `404` que distinguiera las dos diria que en
    /// la otra cuenta hay una subida en curso.
    pub fn find(
        &self,
        account: &str,
        repository: &str,
        id: &str,
    ) -> Option<Arc<tokio::sync::Mutex<UploadSession>>> {
        let mut sessions = self.lock();
        let expired = the_expired(&sessions, self.ttl);
        for expired_id in &expired {
            sessions.remove(expired_id);
        }
        let entry = sessions.get(id)?;
        // La cuenta y el repositorio se comparan **sin** tomar el mutex
        // de la sesion: una sesion que esta siendo escrita no es una
        // sesion que no existe, y un cliente que manda dos trozos a la
        // vez no puede recibir un "no existe" por el hecho de que el
        // otro trozo la tenga tomada.
        if entry.account != account || entry.repository != repository {
            return None;
        }
        Some(Arc::clone(&entry.session))
    }

    /// Cierra una sesion: la saca del registro.
    ///
    /// La sesion que se saca se suelta, y al soltarse su escritura del
    /// CAS se cierra. Con journal el staging es **durable**: soltar el
    /// mango no borra nada —un asiento caducado no es un abandono: la
    /// subida puede seguir en otra réplica—; solo lo que se aborta o se
    /// commitea de verdad toca el almacén.
    pub fn remove(&self, id: &str) {
        self.lock().remove(id);
    }

    /// Cuantas sesiones hay vivas.
    pub fn the_number_of_sessions(&self) -> usize {
        self.lock().len()
    }

    /// El mutex del mapa.
    ///
    /// Un poison aqui significa que un hilo murio con el mutex tomado, y
    /// un registro de sesiones que no se puede leer es un registro que no
    /// puede decir que no: se reinicia.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Un error del journal traducido al protocolo: la autoridad no
/// respondio, y una sesion journalizada no puede volverse invisible.
fn journal_unavailable(error: impl std::fmt::Display) -> OciFailure {
    OciFailure::unavailable(format!("the session journal did not answer: {error}"))
}

/// Un rechazo del almacén al reabrir, traducido al protocolo.
///
/// La sesion **existe** —la autoridad la tiene abierta—: lo que no
/// puede ser es escribir aqui ahora. El `404` mentiria sobre la
/// existencia de la sesion.
fn the_resume_error(error: hodei_hub::blob::BlobError) -> OciFailure {
    match error {
        hodei_hub::blob::BlobError::Unavailable(message) => {
            OciFailure::unavailable(format!("the upload could not be resumed: {message}"))
        }
        error => OciFailure::unavailable(format!("the content store refused the resume: {error}")),
    }
}

/// Una entrada del registro: la sesion y de quien es.
///
/// La cuenta y el repositorio viven **aqui**, y no dentro de la sesion:
/// comprobar de quien es una sesion no tiene que esperar a que la sesion
/// este libre.
struct Entry {
    /// La cuenta que la abrio.
    account: String,
    /// El repositorio al que sube.
    repository: String,
    /// La sesion.
    session: Arc<tokio::sync::Mutex<UploadSession>>,
}

/// Los identificadores de las sesiones que ya caducaron.
fn the_expired(sessions: &HashMap<String, Entry>, ttl: Duration) -> Vec<String> {
    let now = Instant::now();
    sessions
        .iter()
        .filter_map(|(id, entry)| {
            // Una sesion que esta ocupada no caduca: alguien la esta
            // escribiendo, y una escritura en curso no es una sesion
            // abandonada.
            let expired = entry
                .session
                .try_lock()
                .map(|session| now.duration_since(session.last_seen) >= ttl)
                .unwrap_or(false);
            expired.then(|| id.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oci::session_journal::{MemoryJournal, SessionJournal};
    use hodei_hub::blob::{BlobStore, InMemoryBlobStore};

    /// Una reserva de un libro mayor sin limites: las pruebas de
    /// sesiones prueban sesiones, no cuotas.
    fn an_unlimited_ticket() -> crate::quota::QuotaTicket {
        crate::quota::QuotaLedger::new(crate::quota::QuotaLimits::default())
            .begin("acme", None)
            .expect("an unlimited ledger admits everything")
    }

    /// Una sesion que es de otra cuenta no existe para esta.
    #[tokio::test]
    async fn a_session_of_another_account_does_not_exist() {
        let registry = UploadRegistry::new(Duration::from_secs(60), 8);
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let id = session.lock().await.the_id().to_string();

        assert!(registry.find("acme", "releases", &id).is_some());
        assert!(
            registry.find("other", "releases", &id).is_none(),
            "the session of another account was found"
        );
        assert!(
            registry.find("acme", "private", &id).is_none(),
            "the session of another repository was found"
        );
    }

    /// Una sesion que nadie toca caduca, y el sitio queda libre.
    #[tokio::test]
    async fn a_session_that_nobody_touches_expires() {
        let registry = UploadRegistry::new(Duration::from_millis(1), 8);
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let id = session.lock().await.the_id().to_string();

        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(
            registry.find("acme", "releases", &id).is_none(),
            "the expired session was still there"
        );
    }

    /// El almacen de memoria no reanuda: una sesion journalizada suya
    /// conserva el mango local —no hay handle que soltar—.
    #[tokio::test]
    async fn a_session_without_a_handle_keeps_its_writer() {
        let journal = Arc::new(MemoryJournal::new());
        let registry = UploadRegistry::with_journal(Duration::from_secs(60), 8, journal);
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let session = session.lock().await;
        assert!(
            session.the_local_offset().is_some(),
            "a store that cannot resume lost its writer"
        );
        assert!(session.the_handle().is_empty());
    }

    /// Una sesion journalizada de otra cuenta NO se reanima: la
    /// autoridad tiene la sesion, y el ambito se comprueba contra ella.
    #[tokio::test]
    async fn an_alien_session_is_not_resumable() {
        let journal = Arc::new(MemoryJournal::new());
        let registry = UploadRegistry::with_journal(Duration::from_secs(60), 8, journal);
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let id = session.lock().await.the_id().to_string();

        let other = registry
            .find_or_resume("other", "releases", &id, &store)
            .await
            .expect("the journal answers");
        assert!(
            other.is_none(),
            "the session of another account was resumed"
        );
        let other_repo = registry
            .find_or_resume("acme", "private", &id, &store)
            .await
            .expect("the journal answers");
        assert!(
            other_repo.is_none(),
            "the session of another repository was resumed"
        );
        assert!(
            registry
                .the_journal_offset("other", "releases", &id)
                .await
                .expect("the journal answers")
                .is_none(),
            "the offset of an alien session was readable"
        );
    }

    /// El CAS del journal no deja avanzar a un escritor con revision
    /// vieja: es el `416` que evita duplicar un trozo reenviado.
    #[tokio::test]
    async fn the_cas_refuses_a_stale_writer() {
        let journal: Arc<dyn SessionJournal> = Arc::new(MemoryJournal::new());
        let registry =
            UploadRegistry::with_journal(Duration::from_secs(60), 8, Arc::clone(&journal));
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let id = session.lock().await.the_id().to_string();

        // El primer avance entra: v0 -> v1, offset 10.
        match registry.advance(&id, 0, 10).await {
            Advance::Admitted(1) => {}
            other => panic!("the first advance was not admitted: {other:?}"),
        }
        // Un escritor con la revision vieja no mueve la autoridad.
        match registry.advance(&id, 0, 999).await {
            Advance::Conflict(10) => {}
            other => panic!("the stale writer was not refused: {other:?}"),
        }
        // La autoridad sigue en lo que el ganador escribio.
        let record = journal.get(&id).await.expect("journal").expect("record");
        assert_eq!(record.offset, 10);
        assert_eq!(record.version, 1);
    }

    /// Una sesion que esta siendo escrita sigue existiendo.
    ///
    /// Comprobar de quien es una sesion no puede depender de que la
    /// sesion este libre: un cliente que manda dos trozos a la vez --o
    /// que reintenta mientras el otro va-- no puede recibir un "no
    /// existe" por el hecho de que la sesion la tenga tomada otro.
    #[tokio::test]
    async fn a_session_that_is_being_written_is_still_there() {
        let registry = UploadRegistry::new(Duration::from_secs(60), 8);
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let id = session.lock().await.the_id().to_string();

        // La sesion esta tomada, como si un trozo se estuviera
        // escribiendo ahora mismo.
        let taken = session.lock().await;
        assert!(
            registry.find("acme", "releases", &id).is_some(),
            "a session that is being written was not found"
        );
        drop(taken);
    }

    /// El tope de sesiones se respeta: un cliente que abre y abandona
    /// sesiones no puede llenar el registro.
    #[tokio::test]
    async fn the_number_of_sessions_is_capped() {
        let registry = UploadRegistry::new(Duration::from_secs(60), 2);
        let store = InMemoryBlobStore::empty();
        for _ in 0..2 {
            registry
                .open(
                    "acme",
                    "releases",
                    store.begin().await.expect("the session"),
                    an_unlimited_ticket(),
                )
                .await
                .expect("the session opens");
        }
        let error = match registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
        {
            Ok(_) => panic!("the third session does not fit"),
            Err(error) => error,
        };
        assert_eq!(error.the_code(), "BLOB_UPLOAD_INVALID");
    }
}
