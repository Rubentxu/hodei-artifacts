//! Vertical Slice: Create Repository (REPO-T1..T5 scaffold)
//!
//! Objetivos (fase actual REPO-T1):
//! - Definir DTO `CreateRepositoryRequest` alineada con OpenAPI (#/components/schemas/CreateRepositoryRequest).
//! - Definir DTO de respuesta interna (`CreateRepositoryResponse`) que servirá de puente hacia la
//!   representación pública (`Repository` en OpenAPI) sin acoplar controladores HTTP al dominio.
//! - Proveer validación de nombre (regex) preparada para REPO-T3 (aún no se expande el enum de errores).
//!
//! Próximas fases (futuros PR):
//! - REPO-T2: Handler HTTP que parsea JSON -> DTO -> usa un servicio / aplicación y retorna 201.
//! - REPO-T3: Integrar `validate_name` devolviendo error de validación (nuevo `RepositoryError` o
//!            mapeo a ErrorResponse con code INVALID_INPUT).
//! - REPO-T4: Mapear `RepositoryError::DuplicateName` a 409.
//! - REPO-T5: Tests endpoint (happy path / duplicado / inválido).
//!
//! Nota: Sin tests inline (política cero inline tests). Los tests unitarios de validación
//! se ubicarán en `crates/repository/tests/unit/`.

use crate::application::ports::{RepositoryStore, EventBus};
use crate::domain::event::RepositoryCreatedEvent;
use crate::error::RepositoryError;
use anyhow::Result;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use shared::{IsoTimestamp, RepositoryId, UserId};
use std::sync::Arc;
use crate::domain::model::{Repository, RepositoryName, RepositoryDescription};

/// Patrón (alineado con OpenAPI: ^[a-z0-9._-]{3,50}$).
pub const REPO_NAME_PATTERN: &str = r"^[a-z0-9._-]{3,50}$";

lazy_static! {
    static ref REPO_NAME_REGEX: Regex = Regex::new(REPO_NAME_PATTERN).expect("regex repo name");
}

/// DTO de entrada para creación de repositorio (OpenAPI: CreateRepositoryRequest).
#[derive(Debug, Clone, Deserialize)]
pub struct CreateRepositoryRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Resultado interno tras creación (puede mapearse 1:1 a schema Repository).
#[derive(Debug, Clone, Serialize)]
pub struct CreateRepositoryResponse {
    pub id: RepositoryId,
    pub name: String,
    pub description: Option<String>,
    pub created_at: IsoTimestamp,
    pub created_by: UserId,
}

/// Error de validación local (no expuesto todavía en RepositoryError para evitar ampliar superficie
/// hasta REPO-T3).
#[derive(Debug)]
pub enum CreateRepositoryValidationError {
    Empty,
    InvalidPattern,
    TooLong,
    TooShort,
}

impl std::fmt::Display for CreateRepositoryValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use CreateRepositoryValidationError::*;
        match self {
            Empty => write!(f, "nombre vacío"),
            InvalidPattern => write!(f, "nombre no cumple patrón {}", REPO_NAME_PATTERN),
            TooLong => write!(f, "nombre demasiado largo (>50)"),
            TooShort => write!(f, "nombre demasiado corto (<3)"),
        }
    }
}

impl std::error::Error for CreateRepositoryValidationError {}

/// Valida el nombre según reglas de dominio / OpenAPI.
/// De momento se usa localmente; en REPO-T3 se integrará con flujo endpoint.
pub fn validate_name(name: &str) -> Result<(), CreateRepositoryValidationError> {
    if name.is_empty() {
        return Err(CreateRepositoryValidationError::Empty);
    }
    let len = name.len();
    if len < 3 {
        return Err(CreateRepositoryValidationError::TooShort);
    }
    if len > 50 {
        return Err(CreateRepositoryValidationError::TooLong);
    }
    if !REPO_NAME_REGEX.is_match(name) {
        return Err(CreateRepositoryValidationError::InvalidPattern);
    }
    Ok(())
}

/// Construye entidad de dominio a partir del DTO validado.
/// (La validación debe realizarse antes de llamar a esta función.)
pub fn to_domain(
    id: RepositoryId,
    req: &CreateRepositoryRequest,
    user: UserId,
) -> Repository {
    Repository::new(
        id,
        RepositoryName(req.name.clone()),
        req.description
            .as_ref()
            .map(|d| RepositoryDescription(d.clone())),
        user,
    )
}

/// Construye DTO de respuesta desde entidad dominio.
pub fn to_response(repo: &Repository) -> CreateRepositoryResponse {
    CreateRepositoryResponse {
        id: repo.id,
        name: repo.name.0.clone(),
        description: repo.description.as_ref().map(|d| d.0.clone()),
        // Clonar porque IsoTimestamp no implementa Copy.
        created_at: repo.created_at.clone(),
        created_by: repo.created_by,
    }
}

// --- Command and Handler ---

/// Comando para crear un repositorio.
#[derive(Debug, Clone)]
pub struct CreateRepositoryCommand {
    pub name: String,
    pub description: Option<String>,
    pub created_by: UserId,
}

/// Handler para el comando de creación de repositorio.
pub struct CreateRepositoryHandler<S, E>
where
    S: RepositoryStore,
    E: EventBus,
{
    store: Arc<S>,
    event_bus: Arc<E>,
}

impl<S, E> CreateRepositoryHandler<S, E>
where
    S: RepositoryStore,
    E: EventBus,
{
    pub fn new(store: Arc<S>, event_bus: Arc<E>) -> Self {
        Self { store, event_bus }
    }

    /// Ejecuta la lógica de negocio para crear un repositorio.
    pub async fn handle(
        &self,
        cmd: CreateRepositoryCommand,
    ) -> Result<CreateRepositoryResponse, RepositoryError> {
        // 1. Validar
        validate_name(&cmd.name).map_err(|e| RepositoryError::InvalidInput(e.to_string()))?;
        let repo_name = RepositoryName(cmd.name.clone());

        if self.store.find_by_name(&repo_name).await?.is_some() {
            return Err(RepositoryError::DuplicateName);
        }

        // 2. Crear agregado de dominio
        let new_id = RepositoryId::new();
        let request_dto = CreateRepositoryRequest {
            name: cmd.name,
            description: cmd.description,
        };
        let repo = to_domain(new_id, &request_dto, cmd.created_by);

        // 3. Persistir
        self.store.save(&repo).await?;

        // 4. Publicar evento
        let event_payload = RepositoryCreatedEvent {
            repository_id: repo.id,
            name: repo.name.clone(),
            description: repo.description.clone(),
            created_by: repo.created_by,
            occurred_at: repo.created_at.clone(), // Usar el timestamp del agregado
        };
        let envelope = shared::domain::event::DomainEventEnvelope::new_root(event_payload, None);
        self.event_bus.publish(&envelope).await?;

        // 5. Retornar DTO de respuesta
        Ok(to_response(&repo))
    }
}
