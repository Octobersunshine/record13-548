use axum::{Json, extract::State, http::StatusCode};
use crate::models::HealthResponse;
use crate::AppState;

pub async fn health_check(
    State(state): State<AppState>,
) -> (StatusCode, Json<HealthResponse>) {
    let library_size = state.library.len();

    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "ok".to_string(),
            library_size,
        }),
    )
}
