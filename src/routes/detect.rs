use axum::{
    Json,
    extract::{State, Multipart},
    http::StatusCode,
};
use crate::audio::{decode_audio_file, generate_fingerprints, to_mono};
use crate::errors::AppResult;
use crate::models::DetectionResult;
use crate::AppState;
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;

pub async fn detect_infringement(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> AppResult<(StatusCode, Json<DetectionResult>)> {
    let mut audio_data: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| crate::errors::AppError::BadRequest(format!("解析 multipart 失败: {}", e)))?
    {
        let name = field.name().unwrap_or("").to_string();

        if name == "audio" || name == "file" {
            file_name = field.file_name().map(|s| s.to_string());
            let data = field
                .bytes()
                .await
                .map_err(|e| crate::errors::AppError::BadRequest(format!("读取文件失败: {}", e)))?;
            audio_data = Some(data.to_vec());
        }
    }

    let audio_data = audio_data
        .ok_or_else(|| crate::errors::AppError::BadRequest("未找到音频文件".to_string()))?;

    let ext = file_name
        .as_deref()
        .and_then(|n| std::path::Path::new(n).extension().and_then(|e| e.to_str()))
        .unwrap_or("mp3");

    let temp_file = NamedTempFile::new()
        .map_err(|e| crate::errors::AppError::FileError(format!("创建临时文件失败: {}", e)))?;

    let temp_path = temp_file.path();
    let temp_path_with_ext = temp_path.with_extension(ext);

    let mut file = tokio::fs::File::create(&temp_path_with_ext)
        .await
        .map_err(|e| crate::errors::AppError::FileError(format!("创建文件失败: {}", e)))?;

    file.write_all(&audio_data)
        .await
        .map_err(|e| crate::errors::AppError::FileError(format!("写入文件失败: {}", e)))?;
    file.flush()
        .await
        .map_err(|e| crate::errors::AppError::FileError(format!("刷新文件失败: {}", e)))?;
    drop(file);

    let decoded = decode_audio_file(&temp_path_with_ext)?;
    let mono_samples = to_mono(&decoded);
    let fingerprints = generate_fingerprints(&mono_samples, decoded.sample_rate)?;

    let result = state.library.detect(&fingerprints)?;

    let _ = std::fs::remove_file(&temp_path_with_ext);

    Ok((StatusCode::OK, Json(result)))
}

pub async fn detect_infringement_stream(
    State(state): State<AppState>,
    multipart: Multipart,
) -> AppResult<(StatusCode, Json<DetectionResult>)> {
    detect_infringement(State(state), multipart).await
}
