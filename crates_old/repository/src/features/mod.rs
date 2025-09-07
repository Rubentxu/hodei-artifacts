// Features (vertical slices) del bounded context Repository
//
// Cada slice agrupa DTOs, comandos, handlers y lógica específica evitando
// filtraciones entre verticales (Vertical Slice Architecture).
//
// Slices actuales:
// - create_repository (REPO-T1..T5): creación y validación de repositorios.
//
pub mod create_repository;
pub mod get_repository;
pub mod delete_repository;

