use axum::{extract::Path, response::IntoResponse, Extension, Json};
use serde::Serialize;
use tracing::{info, warn};
use uuid::Uuid;

use crate::api::auth::UserIdentity;
use artifact::features::upload_progress::dto::UploadProgressResponse;
use artifact::features::upload_progress::use_case::UploadProgressUseCase;
use artifact::features::upload_progress::{dto::ReceivedChunkInfo, ProgressError};

pub async fn get_progress(
    Extension(use_case): Extension<UploadProgressUseCase>,
    user: UserIdentity,
    Path(upload_id): Path<String>,
) -> impl IntoResponse {
    info!(upload_id = %upload_id, user_id = %user.user_id, "Getting upload progress");

    match use_case.get_progress(&upload_id).await {
        Ok(progress) => {
            if !is_user_authorized(&progress, &user.user_id) {
                warn!(upload_id = %upload_id, user_id = %user.user_id, "Unauthorized access to upload progress");
                return (
                    axum::http::StatusCode::FORBIDDEN,
                    Json(ProgressErrorResponse::unauthorized()),
                )
                    .into_response();
            }

            let response = UploadProgressResponse {
                progress: progress.clone(),
                poll_url: Some(format!("/uploads/{}/progress", upload_id)),
                websocket_url: Some(format!(
                    "ws://localhost:3000/uploads/{}/progress/ws",
                    upload_id
                )),
            };

            (axum::http::StatusCode::OK, Json(response)).into_response()
        }
        Err(ProgressError::SessionNotFound(_)) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(ProgressErrorResponse::not_found()),
        )
            .into_response(),
        Err(ProgressError::AccessDenied(_)) => (
            axum::http::StatusCode::FORBIDDEN,
            Json(ProgressErrorResponse::unauthorized()),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProgressErrorResponse::internal_error()),
        )
            .into_response(),
    }
}

pub async fn list_sessions(
    Extension(use_case): Extension<UploadProgressUseCase>,
    _user: UserIdentity,
) -> impl IntoResponse {
    match use_case.list_sessions().await {
        Ok(sessions) => (axum::http::StatusCode::OK, Json(sessions)).into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProgressErrorResponse::internal_error()),
        )
            .into_response(),
    }
}

pub async fn subscribe_client(
    Extension(use_case): Extension<UploadProgressUseCase>,
    _user: UserIdentity,
    Path(upload_id): Path<String>,
) -> impl IntoResponse {
    let client_id = Uuid::new_v4().to_string();
    match use_case.subscribe_client(&upload_id, &client_id).await {
        Ok(_) => {
            let response = SubscribeResponse {
                client_id,
                upload_id: upload_id.clone(),
                websocket_url: format!("ws://localhost:3000/uploads/{}/progress/ws", upload_id),
            };
            (axum::http::StatusCode::OK, Json(response)).into_response()
        }
        Err(ProgressError::SessionNotFound(_)) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(ProgressErrorResponse::not_found()),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProgressErrorResponse::internal_error()),
        )
            .into_response(),
    }
}

pub async fn unsubscribe_client(
    Extension(use_case): Extension<UploadProgressUseCase>,
    _user: UserIdentity,
    Path(client_id): Path<String>,
) -> impl IntoResponse {
    match use_case.unsubscribe_client(&client_id).await {
        Ok(_) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProgressErrorResponse::internal_error()),
        )
            .into_response(),
    }
}

pub async fn get_received_chunks(
    Extension(use_case): Extension<UploadProgressUseCase>,
    user: UserIdentity,
    Path(upload_id): Path<String>,
) -> impl IntoResponse {
    match use_case.get_received_chunks(&upload_id).await {
        Ok(chunks_response) => {
            if !is_user_authorized_for_upload(&upload_id, &user.user_id) {
                return (
                    axum::http::StatusCode::FORBIDDEN,
                    Json(ProgressErrorResponse::unauthorized()),
                )
                    .into_response();
            }
            (axum::http::StatusCode::OK, Json(chunks_response)).into_response()
        }
        Err(ProgressError::SessionNotFound(_)) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(ProgressErrorResponse::not_found()),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProgressErrorResponse::internal_error()),
        )
            .into_response(),
    }
}

fn is_user_authorized(progress: &artifact::features::upload_progress::dto::UploadProgress, user_id: &str) -> bool {
    progress.upload_id.contains(user_id) || user_id == "admin"
}

fn is_user_authorized_for_upload(upload_id: &str, user_id: &str) -> bool {
    if upload_id.contains(user_id) || user_id == "admin" {
        return true;
    }
    let mut parts = user_id.split('-');
    let prefix = match (parts.next(), parts.next()) {
        (Some(a), Some(b)) => format!("{}-{}", a, b),
        _ => user_id.to_string(),
    };
    upload_id.contains(&prefix)
}

#[derive(Debug, Serialize)]
struct SubscribeResponse {
    client_id: String,
    upload_id: String,
    websocket_url: String,
}
