use crate::audio::{fingerprint::Fingerprint, match_fingerprints};
use crate::errors::{AppError, AppResult};
use crate::models::{
    AudioFingerprintData, CopyrightTrack, DetectionResult, MatchSegment, current_timestamp,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use uuid::Uuid;

const DEFAULT_CONFIDENCE_THRESHOLD: f64 = 0.3;

pub struct CopyrightLibrary {
    tracks: RwLock<HashMap<Uuid, AudioFingerprintData>>,
    hash_index: RwLock<HashMap<u64, Vec<(Uuid, f64)>>>,
    confidence_threshold: f64,
}

impl CopyrightLibrary {
    pub fn new() -> Self {
        Self {
            tracks: RwLock::new(HashMap::new()),
            hash_index: RwLock::new(HashMap::new()),
            confidence_threshold: DEFAULT_CONFIDENCE_THRESHOLD,
        }
    }

    pub fn with_threshold(threshold: f64) -> Self {
        Self {
            tracks: RwLock::new(HashMap::new()),
            hash_index: RwLock::new(HashMap::new()),
            confidence_threshold: threshold,
        }
    }

    pub fn add_track(
        &self,
        title: &str,
        artist: &str,
        fingerprints: &[Fingerprint],
        sample_rate: u32,
        duration: f64,
    ) -> AppResult<CopyrightTrack> {
        let id = Uuid::new_v4();
        let created_at = current_timestamp();

        let hashes: Vec<u64> = fingerprints.iter().map(|fp| fp.hash).collect();
        let timestamps: Vec<f64> = fingerprints.iter().map(|fp| fp.timestamp).collect();

        let track_data = AudioFingerprintData {
            id,
            title: title.to_string(),
            artist: artist.to_string(),
            duration,
            sample_rate,
            fingerprints: hashes.clone(),
            timestamps: timestamps.clone(),
            created_at,
        };

        {
            let mut tracks = self.tracks.write();
            tracks.insert(id, track_data);
        }

        {
            let mut index = self.hash_index.write();
            for (hash, &timestamp) in hashes.iter().zip(timestamps.iter()) {
                index.entry(*hash).or_default().push((id, timestamp));
            }
        }

        Ok(CopyrightTrack {
            id,
            title: title.to_string(),
            artist: artist.to_string(),
            duration,
            fingerprint_count: fingerprints.len(),
            created_at,
        })
    }

    pub fn remove_track(&self, id: &Uuid) -> AppResult<()> {
        let track = {
            let tracks = self.tracks.read();
            tracks.get(id).cloned()
        };

        match track {
            Some(track_data) => {
                {
                    let mut tracks = self.tracks.write();
                    tracks.remove(id);
                }

                {
                    let mut index = self.hash_index.write();
                    for hash in &track_data.fingerprints {
                        if let Some(entries) = index.get_mut(hash) {
                            entries.retain(|(tid, _)| tid != id);
                            if entries.is_empty() {
                                index.remove(hash);
                            }
                        }
                    }
                }

                Ok(())
            }
            None => Err(AppError::NotFound(format!("未找到 ID 为 {} 的曲目", id))),
        }
    }

    pub fn get_track(&self, id: &Uuid) -> Option<CopyrightTrack> {
        let tracks = self.tracks.read();
        tracks.get(id).map(|t| CopyrightTrack {
            id: t.id,
            title: t.title.clone(),
            artist: t.artist.clone(),
            duration: t.duration,
            fingerprint_count: t.fingerprints.len(),
            created_at: t.created_at,
        })
    }

    pub fn list_tracks(&self) -> Vec<CopyrightTrack> {
        let tracks = self.tracks.read();
        tracks
            .values()
            .map(|t| CopyrightTrack {
                id: t.id,
                title: t.title.clone(),
                artist: t.artist.clone(),
                duration: t.duration,
                fingerprint_count: t.fingerprints.len(),
                created_at: t.created_at,
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        let tracks = self.tracks.read();
        tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        let tracks = self.tracks.read();
        tracks.is_empty()
    }

    pub fn detect(&self, query_fingerprints: &[Fingerprint]) -> AppResult<DetectionResult> {
        let start_time = std::time::Instant::now();

        if query_fingerprints.is_empty() {
            return Err(AppError::BadRequest("查询音频没有指纹数据".to_string()));
        }

        let candidate_tracks = self.find_candidates(query_fingerprints);

        let mut best_match: Option<(CopyrightTrack, f64, Vec<(f64, f64)>)> = None;

        for track_id in candidate_tracks {
            let track_fingerprints = {
                let tracks = self.tracks.read();
                if let Some(track) = tracks.get(&track_id) {
                    let fps: Vec<Fingerprint> = track
                        .fingerprints
                        .iter()
                        .zip(track.timestamps.iter())
                        .map(|(h, t)| Fingerprint {
                            hash: *h,
                            timestamp: *t,
                        })
                        .collect();
                    Some((
                        CopyrightTrack {
                            id: track.id,
                            title: track.title.clone(),
                            artist: track.artist.clone(),
                            duration: track.duration,
                            fingerprint_count: track.fingerprints.len(),
                            created_at: track.created_at,
                        },
                        fps,
                    ))
                } else {
                    None
                }
            };

            if let Some((track_info, track_fps)) = track_fingerprints {
                let (confidence, matched_pairs) =
                    match_fingerprints(query_fingerprints, &track_fps);

                if confidence > self.confidence_threshold {
                    match &best_match {
                        Some((_, best_conf, _)) if confidence > *best_conf => {
                            best_match = Some((track_info, confidence, matched_pairs));
                        }
                        None => {
                            best_match = Some((track_info, confidence, matched_pairs));
                        }
                        _ => {}
                    }
                }
            }
        }

        let processing_time_ms = start_time.elapsed().as_millis() as u64;

        let (matched_track, match_segments, confidence) = match best_match {
            Some((track, conf, pairs)) => {
                let segments = extract_match_segments(&pairs);
                (Some(track), segments, conf)
            }
            None => (None, Vec::new(), 0.0),
        };

        let is_infringing = matched_track.is_some();

        Ok(DetectionResult {
            is_infringing,
            confidence,
            matched_track,
            match_segments,
            processing_time_ms,
        })
    }

    fn find_candidates(&self, query_fingerprints: &[Fingerprint]) -> Vec<Uuid> {
        let index = self.hash_index.read();
        let mut candidate_counts: HashMap<Uuid, usize> = HashMap::new();

        for fp in query_fingerprints {
            if let Some(entries) = index.get(&fp.hash) {
                for (track_id, _) in entries {
                    *candidate_counts.entry(*track_id).or_insert(0) += 1;
                }
            }
        }

        let mut candidates: Vec<(Uuid, usize)> = candidate_counts.into_iter().collect();
        candidates.sort_by(|a, b| b.1.cmp(&a.1));

        candidates
            .into_iter()
            .take(10)
            .map(|(id, _)| id)
            .collect()
    }
}

impl Default for CopyrightLibrary {
    fn default() -> Self {
        Self::new()
    }
}

fn extract_match_segments(matched_pairs: &[(f64, f64)]) -> Vec<MatchSegment> {
    if matched_pairs.is_empty() {
        return Vec::new();
    }

    let mut pairs = matched_pairs.to_vec();
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut segments = Vec::new();
    let time_tolerance = 0.5;
    let min_segment_length = 1.0;

    let mut current_segment: Option<(f64, f64, f64, f64, Vec<(f64, f64)>)> = None;

    for &(q_time, t_time) in &pairs {
        match &mut current_segment {
            Some((q_start, q_end, t_start, t_end, points)) => {
                let expected_t = *t_start + (q_time - *q_start);
                if (t_time - expected_t).abs() < time_tolerance {
                    *q_end = q_time;
                    *t_end = t_time;
                    points.push((q_time, t_time));
                } else {
                    let duration = *q_end - *q_start;
                    if duration >= min_segment_length {
                        let confidence = points.len() as f64 / (duration * 10.0).max(1.0);
                        segments.push(MatchSegment {
                            query_start: *q_start,
                            query_end: *q_end,
                            track_start: *t_start,
                            track_end: *t_end,
                            confidence: confidence.min(1.0),
                        });
                    }
                    current_segment = Some((q_time, q_time, t_time, t_time, vec![(q_time, t_time)]));
                }
            }
            None => {
                current_segment = Some((q_time, q_time, t_time, t_time, vec![(q_time, t_time)]));
            }
        }
    }

    if let Some((q_start, q_end, t_start, t_end, points)) = current_segment {
        let duration = q_end - q_start;
        if duration >= min_segment_length {
            let confidence = points.len() as f64 / (duration * 10.0).max(1.0);
            segments.push(MatchSegment {
                query_start: q_start,
                query_end: q_end,
                track_start: t_start,
                track_end: t_end,
                confidence: confidence.min(1.0),
            });
        }
    }

    segments
}
