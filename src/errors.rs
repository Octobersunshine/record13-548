use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("音频解码失败: {0}")]
    AudioDecodeError(String),

    #[error("音频格式不支持: {0}")]
    UnsupportedFormat(String),

    #[error("指纹生成失败: {0}")]
    FingerprintError(String),

    #[error("版权库操作失败: {0}")]
    LibraryError(String),

    #[error("文件操作失败: {0}")]
    FileError(String),

    #[error("请求参数错误: {0}")]
    BadRequest(String),

    #[error("资源不存在: {0}")]
    NotFound(String),

    #[error("内部服务错误: {0}")]
    InternalError(String),
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;
        use serde_json::json;

        let (status, message) = match &self {
            AppError::AudioDecodeError(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::UnsupportedFormat(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::FingerprintError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            AppError::LibraryError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            AppError::FileError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::InternalError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };

        let body = json!({
            "error": status.to_string(),
            "message": message
        });

        (status, axum::Json(body)).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
