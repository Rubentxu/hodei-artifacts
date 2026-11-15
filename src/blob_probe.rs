//! La sonda de integridad del contenido.
//!
//! El catálogo afirma que un asset tiene tales bytes y tal longitud. Esa
//! afirmación es una promesa, y una promesa que nadie comprueba es una
//! mentira: un blob que no esta donde el catálogo dice se sirve como si
//! estuviera sano, y la corrupción se confunde con la ausencia.
//!
//! La sonda comprueba lo **barato**: la longitud que el almacen de
//! bytes tiene de verdad. Recalcular el digest en cada lectura seria leer
//! el contenido entero en cada peticion, y la verificacion profunda —que
//! los bytes sean los que su digest dice— es tarea del scrub, que la
//! hace cuando puede y no en cada peticion.
//!
//! Lo que la sonda NO hace es decidir nada: ella responde con un hecho —
//! esta longitud, o no esta— y el catálogo decide que significa. Por eso
//! vive en la aplicacion, que es donde el catalogo y el almacen de
//! bytes ya se conocen y se cablean.

use std::sync::Arc;

use async_trait::async_trait;

use hodei_catalog::{BlobIntegrityProbe, BlobLocation, BlobProbeError, DigestScope};
use hodei_hub::blob::{BlobError, BlobStore};

/// La sonda que pregunta al almacen de bytes.
///
/// El almacen sabe la longitud de un blob por su digest, y eso es lo que
/// se pregunta: la longitud, que es un metadato, y no el contenido.
pub struct BlobStoreProbe {
    /// El almacen de bytes.
    store: Arc<dyn BlobStore>,
}

impl BlobStoreProbe {
    /// Una sonda sobre el almacen dado.
    pub fn new(store: Arc<dyn BlobStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl BlobIntegrityProbe for BlobStoreProbe {
    async fn length_of(&self, location: &BlobLocation) -> Result<u64, BlobProbeError> {
        match location.scope {
            // El ambito global: el almacen guarda un blob por digest, y
            // la pregunta es por ese digest.
            DigestScope::Global => self
                .store
                .length(&location.digest)
                .await
                .map_err(the_probe_error),
            // El ambito particionado: el almacen de bytes no tiene
            // todavia una ranura por cuenta. La sonda no lo dice "no
            // esta" —eso seria afirmar que el contenido esta roto— ni
            // "no lo se" con la misma respuesta que un fallo de disco:
            // dice que **no lo puede saber**, que es lo cierto. El
            // catalogo lo traduce como indisponible, y el cliente
            // reintenta en vez de borrar algo que no era suyo.
            DigestScope::AccountPartitioned => Err(BlobProbeError::Unavailable {
                detail: "the content store keeps one blob per digest and has no \
                         account-partitioned slot yet"
                    .to_string(),
            }),
        }
    }
}

/// Un fallo del almacen, traducido a lo que la sonda puede decir.
///
/// Un `NotFound` es que el blob no esta: eso si lo sabe el almacen. Todo
/// lo demas —un disco que no responde, un permiso que no deja leer— es
/// que el almacen **no pudo** responder, y la sonda no puede afirmar nada
/// sobre la integridad de un blob que no ha podido mirar.
fn the_probe_error(error: BlobError) -> BlobProbeError {
    match error {
        BlobError::NotFound => BlobProbeError::Missing,
        other => BlobProbeError::Unavailable {
            detail: other.to_string(),
        },
    }
}
