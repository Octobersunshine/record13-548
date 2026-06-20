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
    pub infringement_summary: Option<InfringementSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfringementSummary {
    pub total_infringing_seconds: f64,
    pub total_infringing_ratio: f64,
    pub merged_timeline: Vec<TimelineRange>,
    pub max_confidence: f64,
    pub dominant_scale_factor: f64,
    pub human_readable: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineRange {
    pub start: f64,
    pub end: f64,
    pub start_str: String,
    pub end_str: String,
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchSegment {
    pub raw_query_start: f64,
    pub raw_query_end: f64,
    pub query_start: f64,
    pub query_end: f64,
    pub query_duration: f64,
    pub query_start_str: String,
    pub query_end_str: String,
    pub query_timeline: String,

    pub raw_track_start: f64,
    pub raw_track_end: f64,
    pub track_start: f64,
    pub track_end: f64,
    pub track_duration: f64,
    pub track_start_str: String,
    pub track_end_str: String,
    pub track_timeline: String,

    pub confidence: f64,
    pub boundary_confidence: f64,
    pub scale_factor: f64,
    pub speed_change_description: String,
    pub matched_points_count: usize,
    pub matched_density: f64,
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

pub fn format_seconds(seconds: f64) -> String {
    if seconds < 0.0 {
        return format!("-{}", format_seconds(-seconds));
    }
    let total_secs = seconds.floor() as i64;
    let millis = ((seconds - seconds.floor()) * 100.0).round() as i64;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}.{:02}", hours, minutes, secs, millis)
    } else {
        format!("{:02}:{:02}.{:02}", minutes, secs, millis)
    }
}

pub fn format_seconds_short(seconds: f64) -> String {
    if seconds < 0.0 {
        return format!("-{}", format_seconds_short(-seconds));
    }
    let total_secs = seconds.round() as i64;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{:02}:{:02}", minutes, secs)
    }
}

pub fn describe_scale_factor(scale: f64) -> String {
    if (scale - 1.0).abs() < 0.03 {
        "原速播放".to_string()
    } else if scale < 1.0 {
        let slowdown = ((1.0 - scale) * 100.0).round() as i32;
        format!("慢速播放 (约慢 {}%)", slowdown)
    } else {
        let speedup = ((scale - 1.0) * 100.0).round() as i32;
        format!("加速播放 (约快 {}%)", speedup)
    }
}
