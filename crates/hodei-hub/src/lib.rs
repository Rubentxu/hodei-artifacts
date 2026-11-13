//! El hub de artefactos de confianza: donde Hodei deja de ser un catalogo
//! y pasa a ser el que guarda y distribuye.
//!
//! # La conclusion del SPIKE-010
//!
//! La pregunta del spike era donde la **propiedad nativa del artefacto**
//! da un valor de control-plane que la integracion con un registro
//! externo no da. Y la respuesta es: **la cuarentena y la promocion**.
//!
//! - Un registro externo guarda bytes y los sirve: eso lo hace bien, y
//!   Hodei no lo va a hacer mejor.
//! - Un registro externo **no sabe** si un artefacto es de fiar: no tiene
//!   la decision de confianza de Hodei, y no puede tenerla, porque la
//!   decision viene del grafo de evidencia.
//!
//! O sea: Hodei no compite como almacen.
//! Compite como punto de imposicion.
//!
//! El subconjunto de OCI es el minimo que hace falta para
//! hablar con el mundo, y lo que Hodei anade es el canal de confianza.
//!
//! Y los no-objetivos son explicitos: **Maven y npm no**, hasta que haya
//! demanda, un spike de protocolo y una UAT nueva. Que el codigo viejo
//! exista no es un argumento: el codigo viejo se mira como fuente de
//! conocimiento de dominio y de pruebas, no como coste hundido.
//!
//! # Las tres reglas del hito
//!
//! 1. **Sin decision de confianza no hay promocion.** Un artefacto sin
//!    decision esta en cuarentena, y la cuarentena es el estado POR
//!    DEFECTO de un artefacto recién admitido.
//! 2. **Promocionar no muta los bytes.** El digest es la identidad del
//!    contenido, y la promocion cambia el canal, no lo que hay dentro.
//! 3. **El mismo contenido no se guarda dos veces.** La identidad del
//!    contenido es su digest, y al leer se vuelve a calcular y se
//!    comprueba.
//!
//! Y lo que Hodei **no** hace: hablar de protocolo entero. El subconjunto
//! soportado de OCI esta declarado en el codigo (`oci::UNSUPPORTED`), y lo
//! que no esta en la lista no se sirve.

#![forbid(unsafe_code)]

pub mod blob;
pub mod hub;
pub mod oci;

pub use blob::{BlobError, BlobStore, FileBlobStore, InMemoryBlobStore};
pub use hub::{
    ArtifactHub, Availability, HubArtifact, PromotionAudit, PromotionError, TrustVerdict,
};
pub use oci::{OciDescriptor, OciError, OciManifest, OciSupport};
