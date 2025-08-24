use std::sync::Arc;
use axum::{Extension, extract::Json, http::StatusCode};
use uuid::Uuid;

use crate::application::ports::RepositoryStore; // Importa el trait para habilitar métodos (save)
use crate::infrastructure::MongoRepositoryStore;
use crate::features::create_repository::{
    CreateRepositoryRequest,
    CreateRepositoryResponse,
    validate_name,
    to_domain,
    to_response,
};
use shared::{RepositoryId, UserId};

/// Código de error simplificado (REPO-T3/REPO-T4 lo alineará 100% con OpenAPI ErrorResponse.code)
#[derive(serde::Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidInput,
    RepositoryConflict,
    InternalError,
}

#[derive(serde::Serialize)]
pub struct ErrorResponseBody {
    error: String,
    code: ErrorCode,
}

/// Handler POST /v1/repositories (REPO-T2 alcance parcial).
///
/// Flujo:
/// 1. Deserializa JSON a CreateRepositoryRequest.
/// 2. Valida nombre (validate_name).
/// 3. Construye entidad y persiste vía RepositoryStore (MongoRepositoryStore).
/// 4. Devuelve 201 con representación Repository (CreateRepositoryResponse).
///
/// Pendiente:
/// - REPO-T3: Mapping completo errores de validación → INVALID_INPUT con details.
/// - REPO-T4: Mapping DuplicateName definitivo a REPOSITORY_CONFLICT (código OpenAPI).
/// - Reemplazar UserId placeholder por usuario autenticado (IAM).
pub async fn create_repository_handler(
    Extension(store): Extension<Arc<MongoRepositoryStore>>,
    Json(req): Json<CreateRepositoryRequest>,
) -> Result<(StatusCode, Json<CreateRepositoryResponse>), (StatusCode, Json<ErrorResponseBody>)> {
    if let Err(e) = validate_name(&req.name) {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponseBody {
                error: e.to_string(),
                code: ErrorCode::InvalidInput,
            }),
        ));
    }

    let user_id = UserId(Uuid::nil()); // TODO: inyectar desde capa IAM
    let id = RepositoryId(Uuid::new_v4());
    let repo = to_domain(id, &req, user_id);

    if let Err(e) = store.save(&repo).await {
        return match e {
            crate::error::RepositoryError::DuplicateName => Err((
                StatusCode::CONFLICT,
                Json(ErrorResponseBody {
                    error: "nombre duplicado".into(),
                    code: ErrorCode::RepositoryConflict,
                }),
            )),
            other => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponseBody {
                    error: format!("error interno: {other}"),
                    code: ErrorCode::InternalError,
                }),
            )),
        };
    }

    Ok((StatusCode::CREATED, Json(to_response(&repo))))
}
