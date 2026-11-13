//! El prototipo de la proyeccion analitica (SPIKE-003).
//!
//! Vive en su propia crate por una razon medida: la libreria precompilada
//! de LadybugDB trae su propio zstd y no convive con el de SurrealDB en el
//! mismo binario. Una proyeccion secundaria es un proceso aparte, y el
//! prototipo tiene que parecerse a lo que seria.

#![forbid(unsafe_code)]
