use audio_copyright_detector::{AppState, routes::create_routes};
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "audio_copyright_detector=debug,tower_http=debug,axum=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = AppState::new();

    let app = create_routes(state.clone()).layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any),
    );

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("服务启动中，监听地址: {}", addr);
    tracing::info!("健康检查: http://{}/api/health", addr);
    tracing::info!("版权库管理: http://{}/api/library", addr);
    tracing::info!("侵权检测: http://{}/api/detect", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
