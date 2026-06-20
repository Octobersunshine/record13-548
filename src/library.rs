use crate::audio::{fingerprint::Fingerprint, sliding_window_match, extract_segments, build_timeline_summary, SegmentMatch};
use crate::errors::{AppError, AppResult};
use crate::models::{
    CopyrightTrack, DetectionResult, InfringementSummary, MatchSegment, TimelineRange,
    current_timestamp, describe_scale_factor, format_seconds, format_seconds_short,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use uuid::Uuid;

const DEFAULT_CONFIDENCE_THRESHOLD: f64 = 0.15;

#[derive(Clone)]
struct StoredFingerprint {
    freq_hash: u64,
    hash: u64,
    timestamp: f64,
    frame_idx: u32,
    f1: u16,
    f2: u16,
    dt_bucket: u8,
}

impl From<&Fingerprint> for StoredFingerprint {
    fn from(fp: &Fingerprint) -> Self {
        Self {
            freq_hash: fp.freq_hash,
            hash: fp.hash,
            timestamp: fp.timestamp,
            frame_idx: fp.frame_idx as u32,
            f1: fp.f1,
            f2: fp.f2,
            dt_bucket: fp.dt_bucket,
        }
    }
}

impl StoredFingerprint {
    fn to_fingerprint(&self) -> Fingerprint {
        Fingerprint {
            freq_hash: self.freq_hash,
            hash: self.hash,
            timestamp: self.timestamp,
            frame_idx: self.frame_idx as usize,
            f1: self.f1,
            f2: self.f2,
            dt_bucket: self.dt_bucket,
        }
    }
}

pub struct CopyrightLibrary {
    tracks: RwLock<HashMap<Uuid, TrackData>>,
    freq_index: RwLock<HashMap<u64, Vec<(Uuid, f64, usize)>>>,
    hash_index: RwLock<HashMap<u64, Vec<(Uuid, f64)>>>,
    confidence_threshold: f64,
}

struct TrackData {
    id: Uuid,
    title: String,
    artist: String,
    duration: f64,
    #[allow(dead_code)]
    sample_rate: u32,
    created_at: u64,
    fingerprints: Vec<StoredFingerprint>,
}

impl CopyrightLibrary {
    pub fn new() -> Self {
        Self {
            tracks: RwLock::new(HashMap::new()),
            freq_index: RwLock::new(HashMap::new()),
            hash_index: RwLock::new(HashMap::new()),
            confidence_threshold: DEFAULT_CONFIDENCE_THRESHOLD,
        }
    }

    pub fn with_threshold(threshold: f64) -> Self {
        Self {
            tracks: RwLock::new(HashMap::new()),
            freq_index: RwLock::new(HashMap::new()),
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

        let stored: Vec<StoredFingerprint> = fingerprints.iter().map(|f| f.into()).collect();

        let track_data = TrackData {
            id,
            title: title.to_string(),
            artist: artist.to_string(),
            duration,
            sample_rate,
            created_at,
            fingerprints: stored,
        };

        {
            let mut tracks = self.tracks.write();
            tracks.insert(id, track_data);
        }

        {
            let mut freq_idx = self.freq_index.write();
            let mut hash_idx = self.hash_index.write();
            let tracks = self.tracks.read();
            let track = tracks.get(&id).unwrap();

            for (frame_offset, sfp) in track.fingerprints.iter().enumerate() {
                freq_idx
                    .entry(sfp.freq_hash)
                    .or_default()
                    .push((id, sfp.timestamp, frame_offset));
                hash_idx
                    .entry(sfp.hash)
                    .or_default()
                    .push((id, sfp.timestamp));
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
        let fingerprints_to_remove: Vec<(u64, u64)> = {
            let tracks = self.tracks.read();
            if let Some(track) = tracks.get(id) {
                track
                    .fingerprints
                    .iter()
                    .map(|f| (f.freq_hash, f.hash))
                    .collect()
            } else {
                return Err(AppError::NotFound(format!(
                    "未找到 ID 为 {} 的曲目",
                    id
                )));
            }
        };

        {
            let mut tracks = self.tracks.write();
            tracks.remove(id);
        }

        {
            let mut freq_idx = self.freq_index.write();
            let mut hash_idx = self.hash_index.write();

            for (freq_hash, hash) in &fingerprints_to_remove {
                if let Some(entries) = freq_idx.get_mut(freq_hash) {
                    entries.retain(|(tid, _, _)| tid != id);
                    if entries.is_empty() {
                        freq_idx.remove(freq_hash);
                    }
                }
                if let Some(entries) = hash_idx.get_mut(hash) {
                    entries.retain(|(tid, _)| tid != id);
                    if entries.is_empty() {
                        hash_idx.remove(hash);
                    }
                }
            }
        }

        Ok(())
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

        let query_duration = query_fingerprints
            .last()
            .map(|fp| fp.timestamp)
            .unwrap_or(0.0);

        let candidate_tracks = self.find_candidates_enhanced(query_fingerprints);

        if candidate_tracks.is_empty() {
            let processing_time_ms = start_time.elapsed().as_millis() as u64;
            return Ok(DetectionResult {
                is_infringing: false,
                confidence: 0.0,
                matched_track: None,
                match_segments: Vec::new(),
                processing_time_ms,
                infringement_summary: None,
            });
        }

        let mut best_match: Option<(CopyrightTrack, crate::audio::MatchResult, Vec<SegmentMatch>)> =
            None;

        for (track_id, _score) in candidate_tracks {
            let result_opt = {
                let tracks = self.tracks.read();
                if let Some(track) = tracks.get(&track_id) {
                    let track_fps: Vec<Fingerprint> = track
                        .fingerprints
                        .iter()
                        .map(|sfp| sfp.to_fingerprint())
                        .collect();

                    let track_info = CopyrightTrack {
                        id: track.id,
                        title: track.title.clone(),
                        artist: track.artist.clone(),
                        duration: track.duration,
                        fingerprint_count: track.fingerprints.len(),
                        created_at: track.created_at,
                    };

                    let match_result = sliding_window_match(query_fingerprints, &track_fps);
                    let segments =
                        extract_segments(&match_result.consistent_pairs, match_result.scale_factor);
                    Some((track_info, match_result, segments))
                } else {
                    None
                }
            };

            if let Some((track_info, match_result, segments)) = result_opt {
                let match_conf = match_result.confidence;
                let segment_score = if segments.is_empty() {
                    0.0
                } else {
                    segments
                        .iter()
                        .map(|s| s.confidence * (s.query_end - s.query_start))
                        .sum::<f64>()
                        / segments
                            .iter()
                            .map(|s| (s.query_end - s.query_start).max(0.1))
                            .sum::<f64>()
                };
                let combined_conf = (match_conf * 0.5 + segment_score * 0.5).min(1.0);

                if combined_conf > self.confidence_threshold {
                    let is_better = match &best_match {
                        Some((_, best_mr, best_segs)) => {
                            let best_seg_score = if best_segs.is_empty() {
                                0.0
                            } else {
                                best_segs
                                    .iter()
                                    .map(|s| s.confidence * (s.query_end - s.query_start))
                                    .sum::<f64>()
                                    / best_segs
                                        .iter()
                                        .map(|s| (s.query_end - s.query_start).max(0.1))
                                        .sum::<f64>()
                            };
                            let best_combined =
                                (best_mr.confidence * 0.5 + best_seg_score * 0.5).min(1.0);
                            combined_conf > best_combined
                        }
                        None => true,
                    };

                    if is_better {
                        best_match = Some((track_info, match_result, segments));
                    }
                }
            }
        }

        let processing_time_ms = start_time.elapsed().as_millis() as u64;

        match best_match {
            Some((track, mr, segments)) => {
                let model_segments: Vec<MatchSegment> = segments
                    .iter()
                    .map(|s| {
                        let q_dur = s.query_end - s.query_start;
                        let t_dur = s.target_end - s.target_start;
                        MatchSegment {
                            raw_query_start: s.raw_query_start,
                            raw_query_end: s.raw_query_end,
                            query_start: s.query_start,
                            query_end: s.query_end,
                            query_duration: q_dur,
                            query_start_str: format_seconds(s.query_start),
                            query_end_str: format_seconds(s.query_end),
                            query_timeline: format!(
                                "{} - {}",
                                format_seconds_short(s.query_start),
                                format_seconds_short(s.query_end)
                            ),

                            raw_track_start: s.raw_target_start,
                            raw_track_end: s.raw_target_end,
                            track_start: s.target_start,
                            track_end: s.target_end,
                            track_duration: t_dur,
                            track_start_str: format_seconds(s.target_start),
                            track_end_str: format_seconds(s.target_end),
                            track_timeline: format!(
                                "{} - {}",
                                format_seconds_short(s.target_start),
                                format_seconds_short(s.target_end)
                            ),

                            confidence: s.confidence,
                            boundary_confidence: s.boundary_confidence,
                            scale_factor: s.scale_factor,
                            speed_change_description: describe_scale_factor(s.scale_factor),
                            matched_points_count: s.matched_points_count,
                            matched_density: s.matched_density,
                        }
                    })
                    .collect();

                let avg_seg_conf = if model_segments.is_empty() {
                    0.0
                } else {
                    model_segments
                        .iter()
                        .map(|s| s.confidence)
                        .sum::<f64>()
                        / model_segments.len() as f64
                };

                let merged_conf = (mr.confidence * 0.5 + avg_seg_conf * 0.5).min(1.0);

                let track_duration = track.duration;
                let (total_infringing_seconds, infringement_ratio, max_conf, timeline_ranges) =
                    build_timeline_summary(&segments, query_duration, track_duration);

                let merged_timeline: Vec<TimelineRange> = timeline_ranges
                    .iter()
                    .map(|&(s, e)| TimelineRange {
                        start: s,
                        end: e,
                        start_str: format_seconds(s),
                        end_str: format_seconds(e),
                        duration: e - s,
                    })
                    .collect();

                let dominant_scale = if segments.is_empty() {
                    1.0
                } else {
                    let weighted_sum: f64 = segments
                        .iter()
                        .map(|s| s.scale_factor * s.matched_points_count as f64)
                        .sum();
                    let total_weight: f64 = segments
                        .iter()
                        .map(|s| s.matched_points_count as f64)
                        .sum();
                    if total_weight > 0.0 {
                        weighted_sum / total_weight
                    } else {
                        1.0
                    }
                };

                let human_readable = if merged_timeline.is_empty() {
                    format!(
                        "检测到匹配《{} - {}》，置信度 {:.1}%",
                        track.title,
                        track.artist,
                        merged_conf * 100.0
                    )
                } else {
                    let ranges_str: Vec<String> = merged_timeline
                        .iter()
                        .map(|r| {
                            format!(
                                "[{} → {}] ({:.1}秒)",
                                format_seconds_short(r.start),
                                format_seconds_short(r.end),
                                r.duration
                            )
                        })
                        .collect();
                    format!(
                        "视频中 {:.1}% 的内容疑似侵权（总时长 {:.1}秒），匹配曲目《{} - {}》。\n  侵权时间段：{}\n  播放速度：{}，置信度 {:.1}%",
                        infringement_ratio * 100.0,
                        total_infringing_seconds,
                        track.title,
                        track.artist,
                        ranges_str.join(" "),
                        describe_scale_factor(dominant_scale),
                        merged_conf * 100.0
                    )
                };

                let summary = if !model_segments.is_empty() {
                    Some(InfringementSummary {
                        total_infringing_seconds,
                        total_infringing_ratio: infringement_ratio,
                        merged_timeline,
                        max_confidence: (max_conf.max(merged_conf)).min(1.0),
                        dominant_scale_factor: dominant_scale,
                        human_readable,
                    })
                } else {
                    None
                };

                Ok(DetectionResult {
                    is_infringing: true,
                    confidence: merged_conf,
                    matched_track: Some(track),
                    match_segments: model_segments,
                    processing_time_ms,
                    infringement_summary: summary,
                })
            }
            None => Ok(DetectionResult {
                is_infringing: false,
                confidence: 0.0,
                matched_track: None,
                match_segments: Vec::new(),
                processing_time_ms,
                infringement_summary: None,
            }),
        }
    }

    fn find_candidates_enhanced(&self, query_fingerprints: &[Fingerprint]) -> Vec<(Uuid, f64)> {
        let freq_idx = self.freq_index.read();
        let hash_idx = self.hash_index.read();

        let mut freq_scores: HashMap<Uuid, (usize, usize)> = HashMap::new();

        for fp in query_fingerprints {
            if let Some(entries) = freq_idx.get(&fp.freq_hash) {
                for (track_id, _, _) in entries {
                    let entry = freq_scores.entry(*track_id).or_insert((0, 0));
                    entry.0 += 1;
                }
            }

            if let Some(entries) = hash_idx.get(&fp.hash) {
                for (track_id, _) in entries {
                    let entry = freq_scores.entry(*track_id).or_insert((0, 0));
                    entry.1 += 1;
                }
            }
        }

        let query_len = query_fingerprints.len() as f64;
        let mut scored: Vec<(Uuid, f64)> = freq_scores
            .into_iter()
            .map(|(tid, (freq_matches, hash_matches))| {
                let score = (freq_matches as f64 * 1.0 + hash_matches as f64 * 2.5)
                    / query_len.max(1.0);
                (tid, score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored.into_iter().take(8).collect()
    }
}

impl Default for CopyrightLibrary {
    fn default() -> Self {
        Self::new()
    }
}
