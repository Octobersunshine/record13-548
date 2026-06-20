use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFingerprintData {
    pub id: Uuid,
    pub title: String,
    pub artist: String,
    pub duration: f64,
    pub sample_rate: u32,
    pub fingerprints: Vec<u64>,
    pub timestamps: Vec<f64>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyrightTrack {
    pub id: Uuid,
    pub title: String,
    pub artist: String,
    pub duration: f64,
    pub fingerprint_count: usize,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    pub is_infringing: bool,
    pub confidence: f64,
    pub matched_track: Option<CopyrightTrack>,
    pub match_segments: Vec<MatchSegment>,
    pub processing_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchSegment {
    pub query_start: f64,
    pub query_end: f64,
    pub track_start: f64,
    pub track_end: f64,
    pub confidence: f64,
    pub scale_factor: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResponse {
    pub track_id: Uuid,
    pub title: String,
    pub artist: String,
    pub duration: f64,
    pub fingerprint_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddTrackQuery {
    pub title: String,
    pub artist: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryListResponse {
    pub total: usize,
    pub tracks: Vec<CopyrightTrack>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub library_size: usize,
}

pub fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
