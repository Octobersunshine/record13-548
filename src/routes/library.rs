use axum::{
    Json,
    extract::{Path, State, Multipart},
    http::StatusCode,
};
use crate::audio::{decode_audio_file, generate_fingerprints, to_mono};
use crate::errors::AppResult;
use crate::models::{
    AddTrackQuery, CopyrightTrack, LibraryListResponse, UploadResponse,
};
use crate::AppState;
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

pub async fn list_tracks(
    State(state): State<AppState>,
) -> (StatusCode, Json<LibraryListResponse>) {
    let tracks = state.library.list_tracks();
    let total = tracks.len();

    (
        StatusCode::OK,
        Json(LibraryListResponse { total, tracks }),
    )
}

pub async fn get_track(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<(StatusCode, Json<CopyrightTrack>)> {
    match state.library.get_track(&id) {
        Some(track) => Ok((StatusCode::OK, Json(track))),
        None => Err(crate::errors::AppError::NotFound(format!(
            "未找到 ID 为 {} 的曲目",
            id
        ))),
    }
}

pub async fn add_track(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<AddTrackQuery>,
    mut multipart: Multipart,
) -> AppResult<(StatusCode, Json<UploadResponse>)> {
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

    let title = if query.title.is_empty() {
        file_name.unwrap_or_else(|| "未命名".to_string())
    } else {
        query.title
    };

    let artist = query.artist.unwrap_or_else(|| "未知艺术家".to_string());

    let track = state.library.add_track(
        &title,
        &artist,
        &fingerprints,
        decoded.sample_rate,
        decoded.duration,
    )?;

    let _ = std::fs::remove_file(&temp_path_with_ext);

    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            track_id: track.id,
            title: track.title,
            artist: track.artist,
            duration: track.duration,
            fingerprint_count: track.fingerprint_count,
        }),
    ))
}

pub async fn delete_track(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    state.library.remove_track(&id)?;
    Ok(StatusCode::NO_CONTENT)
}
