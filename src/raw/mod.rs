//! La superficie de contenido **crudo**.
//!
//! Un repositorio crudo no tiene paquetes, ni versiones, ni formatos:
//! tiene **bytes en una ruta**. Por eso la superficie es pequeña a
//! propósito: `PUT` publica, `GET` descarga, `HEAD` describe, y las tres
//! hablando con un repositorio `hosted`.
//!
//! Lo que esta superficie **no** hace:
//!
//! - No comparte semantica con OCI, Maven ni npm: un repositorio de otro
//!   formato no se sirve aquí, y una ruta de otro formato no significa
//!   nada aquí.
//! - No decide la autorización por su cuenta: pide la decisión de Cedar
//!   con la acción que corresponde, y si la decisión no es permiso, no
//!   hay contenido.
//! - No admite una ruta ambigua: las reglas están en
//!   [`path`], y se aplican antes de tocar el almacén de contenido.

pub mod conditional;
pub mod path;
pub mod policy;
pub mod range;
