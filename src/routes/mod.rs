pub mod health;
pub mod library;
pub mod detect;

use axum::Router;
use crate::AppState;

pub fn create_routes(state: AppState) -> Router {
    Router::new()
        .nest("/api/health", health_routes())
        .nest("/api/library", library_routes())
        .nest("/api/detect", detect_routes())
        .with_state(state)
}

fn health_routes() -> Router<AppState> {
    Router::new()
        .route("/", axum::routing::get(health::health_check))
}

fn library_routes() -> Router<AppState> {
    Router::new()
        .route("/", axum::routing::get(library::list_tracks))
        .route("/", axum::routing::post(library::add_track))
        .route("/:id", axum::routing::get(library::get_track))
        .route("/:id", axum::routing::delete(library::delete_track))
}

fn detect_routes() -> Router<AppState> {
    Router::new()
        .route("/", axum::routing::post(detect::detect_infringement))
        .route("/stream", axum::routing::post(detect::detect_infringement_stream))
}
