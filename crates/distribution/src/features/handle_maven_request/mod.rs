// crates/distribution/src/features/handle_maven_request/mod.rs

//! Feature: Handle Maven Request
//! 
//! Responsabilidad: Procesar requests HTTP Maven (GET/PUT) para artefactos
//! 
//! Este feature es completamente independiente con sus propios puertos segregados
//! siguiendo el principio de segregación de interfaces (ISP).

pub mod dto;
pub mod ports;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod di;

// Solo exportar lo necesario al exterior
pub use dto::{MavenGetArtifactRequest, MavenGetArtifactResponse, MavenPutArtifactRequest, MavenPutArtifactResponse};