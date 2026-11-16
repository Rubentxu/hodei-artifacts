//! Las sesiones de subida: el estado que vive entre peticiones.
//!
//! El protocolo de subida es **por trozos**: un `POST` abre la sesion, un
//! `PATCH` (o varios) la llenan y un `PUT` la cierra. Entre una peticion
//! y otra hay estado que hay que recordar: cuantos bytes han entrado, que
//! sesion de escritura del CAS los esta guardando, de que cuenta y de que
//! repositorio es la subida.
//!
//! Ese estado vive aqui, en memoria, y por tres razones:
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
//! Lo que **no** se guarda aqui es el contenido: los bytes van al CAS por
//! su sesion de escritura, y el staging de cada subida es un fichero
//! propio, para que dos subidas del mismo contenido no se pisen.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hodei_hub::blob::BlobWriteSession;

use crate::oci::failure::OciFailure;

/// Una sesion de subida en curso.
///
/// El escritor es la sesion de escritura del CAS: los bytes van a su
/// staging, y el commit es lo unico que publica.
pub struct UploadSession {
    /// El identificador opaco de la sesion.
    id: String,
    /// La escritura en curso del almacen de contenido.
    writer: Box<dyn BlobWriteSession>,
    /// Cuando se toco por ultima vez.
    last_seen: Instant,
}

impl UploadSession {
    /// El identificador de la sesion.
    pub fn the_id(&self) -> &str {
        &self.id
    }

    /// Los bytes que han entrado.
    pub fn bytes_written(&self) -> u64 {
        self.writer.bytes_written()
    }

    /// El escritor de la sesion, para escribir y para cerrar.
    pub fn the_writer(&mut self) -> &mut Box<dyn BlobWriteSession> {
        &mut self.writer
    }

    /// Anota que la sesion se ha tocado.
    ///
    /// La caducidad se cuenta desde el ultimo uso, no desde el
    /// principio: una subida larga vive mientras alguien la escribe, y
    /// una sesion que nadie toca es una sesion que hay que olvidar.
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
    }
}

/// El registro de las sesiones de subida.
///
/// El mapa esta bajo un mutex **sync** porque sus operaciones son cortas
/// y no esperan: lo que espera es la sesion, y cada sesion tiene su
/// propio mutex asincrono. Dos subidas a la vez no se bloquean entre si.
pub struct UploadRegistry {
    /// Las sesiones vivas, por identificador.
    sessions: Mutex<HashMap<String, Entry>>,
    /// Cuanto vive una sesion sin que nadie la toque.
    ttl: Duration,
    /// Cuantas sesiones pueden vivir a la vez.
    max_sessions: usize,
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
    /// Un registro vacio.
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
        }
    }

    /// Abre una sesion y la registra.
    ///
    /// El identificador es un UUID: lo que el cliente recibe en el
    /// `Location` y lo que devuelve en cada peticion siguiente.
    pub async fn open(
        &self,
        account: &str,
        repository: &str,
        writer: Box<dyn BlobWriteSession>,
    ) -> Result<Arc<tokio::sync::Mutex<UploadSession>>, OciFailure> {
        let mut sessions = self.lock();
        // Las sesiones caducadas se van antes de contar: un registro que
        // cuenta las muertas no puede saber si cabe una mas.
        let expired = the_expired(&sessions, self.ttl);
        for id in &expired {
            sessions.remove(id);
        }
        if sessions.len() >= self.max_sessions {
            return Err(OciFailure::blob_upload_invalid(format!(
                "there are already {} uploads in progress",
                self.max_sessions
            )));
        }
        let id = uuid::Uuid::new_v4().hyphenated().to_string();
        let session = Arc::new(tokio::sync::Mutex::new(UploadSession {
            id: id.clone(),
            writer,
            last_seen: Instant::now(),
        }));
        sessions.insert(
            id,
            Entry {
                account: account.to_string(),
                repository: repository.to_string(),
                session: Arc::clone(&session),
            },
        );
        Ok(session)
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
    /// La sesion que se saca se suelta, y al soltarse su escritura del CAS
    /// se cierra. Si estaba commiteada, el staging ya no esta; si no, el
    /// staging se borra.
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
    use hodei_hub::blob::{BlobStore, InMemoryBlobStore};

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
                )
                .await
                .expect("the session opens");
        }
        let error = match registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
            )
            .await
        {
            Ok(_) => panic!("the third session does not fit"),
            Err(error) => error,
        };
        assert_eq!(error.the_code(), "BLOB_UPLOAD_INVALID");
    }
}
