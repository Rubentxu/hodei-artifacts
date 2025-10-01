use crate::app_state::AppState;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::{sync::Arc, time::Instant};

pub async fn metrics_middleware(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let start = Instant::now();
    
    // Record request
    state.metrics.record_request();
    
    let response = next.run(request).await;
    
    // Record duration
    let duration = start.elapsed();
    state.metrics.record_request_duration(duration);
    
    // Record errors if any
    if response.status().is_server_error() {
        state.metrics.record_error("server_error");
    } else if response.status().is_client_error() {
        state.metrics.record_error("client_error");
    }
    
    response
}
