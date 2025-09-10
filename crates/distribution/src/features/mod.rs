// crates/distribution/src/features/mod.rs

//! Features del crate distribution siguiendo Vertical Slice Architecture (VSA)
//! Cada feature es completamente independiente con sus propios puertos segregados

pub mod handle_maven_request;
pub mod handle_npm_request;
pub mod handle_docker_request;
pub mod generate_maven_metadata;
pub mod generate_npm_metadata;
pub mod generate_docker_manifest;