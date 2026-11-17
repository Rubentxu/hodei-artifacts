//! El backend S3-compatible del almacen de contenido.
//!
//! Hodei posee los bytes; el sitio donde los posee puede ser un directorio
//! o un Object Store que hable el protocolo S3. Este crate **no** añade un
//! tercer almacen: implementa el unico puerto [`hodei_hub::blob::BlobStore`]
//! sobre `aws-sdk-s3`, de modo que el dominio no sepa nunca si detras hay
//! AWS, MinIO, R2 o Ceph.
//!
//! # Las decisiones del backend
//!
//! - **La clave de un blob es su digest**: `blobs/<ab>/<cd>/<hex>`, el mismo
//!   reparto que el almacen de ficheros. En un CAS la clave es la identidad,
//!   y la identidad es el contenido: publicar dos veces el mismo contenido
//!   no corrompe nada, porque los bytes del ultimo son los bytes del primero.
//! - **La sesion de escritura es una subida multiple**: los trozos suben en
//!   cuanto llenan `part_size` y con su `Content-MD5`, y la memoria del
//!   proceso esta acotada por `part_size * max_inflight_parts`. El commit
//!   **no completa nada que no cuadre**: si el largo o el digest discrepan
//!   de lo declarado, la subida se aborta y no queda ninguna referencia
//!   visible. Esa es la condicion de integridad de la finalizacion.
//! - **El recolector vive de `Last-Modified`**: `list_blobs` recorre el
//!   prefijo paginado y `delete_if_older_than` mira la edad antes de borrar,
//!   con la misma barrera entre publicar y recolectar que el almacen de
//!   ficheros. Las leases y los tags referidos los respeta el servicio de
//!   GC, que es quien decide; este backend solo cumple su parte: no borrar
//!   lo que se publico hace nada.
//! - **Las credenciales no pasan por aqui**: el almacen recibe un cliente
//!   ya construido por el despliegue, con la cadena de proveedores segura
//!   del SDK (variables de entorno, fichero compartido, rol de instancia).
//!   Nada del almacen guarda, deriva ni imprime un secreto, y los errores
//!   que produce tampoco: el `Debug` del almacen muestra la configuracion
//!   publica y nunca el cliente.
//!
//! # El presupuesto de memoria y el benchmark
//!
//! El presupuesto se declara, no se descubre: una sesion en curso puede
//! tener `max_inflight_parts` trozos de `part_size` bytes simultaneamente
//! en transmision, y ese producto es el techo de memoria por sesion. Con
//! los valores por defecto (8 MiB x 4) son 32 MiB, frente a la via de
//! `put`, que recibe el contenido entero en memoria por contrato del
//! puerto. `tests/hsp116_s3_bench.rs` comprueba el techo con un blob seis
//! veces mayor que el presupuesto midiendo la memoria residente del
//! proceso —el servidor S3 de pruebas vive en otro proceso, en un
//! contenedor LocalStack— y deja los numeros comparativos con el almacen
//! de ficheros.

mod store;

pub use store::{S3BlobConfig, S3BlobConfigError, S3BlobStore};
