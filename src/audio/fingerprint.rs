use crate::errors::{AppError, AppResult};
use rustfft::{FftPlanner, num_complex::Complex};
use std::collections::HashMap;

pub const FINGERPRINT_SAMPLE_RATE: u32 = 22050;
pub const FRAME_SIZE: usize = 2048;
pub const HOP_SIZE: usize = 512;
pub const FINGERPRINT_BINS: usize = 64;
pub const FAN_VALUE: usize = 15;
pub const MIN_FREQ_BIN: usize = 0;
pub const MAX_FREQ_BIN: usize = 63;

const SCALE_FACTORS: &[f64] = &[
    0.70, 0.75, 0.80, 0.85, 0.90, 0.95,
    1.00,
    1.05, 1.10, 1.15, 1.20, 1.30, 1.40, 1.50,
];

const DT_BUCKETS: &[usize] = &[1, 2, 3, 5, 8, 12, 18, 26, 40];

#[derive(Debug, Clone)]
pub struct Fingerprint {
    pub hash: u64,
    pub freq_hash: u64,
    pub timestamp: f64,
    pub frame_idx: usize,
    pub f1: u16,
    pub f2: u16,
    pub dt_bucket: u8,
}

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub confidence: f64,
    pub scale_factor: f64,
    pub offset: f64,
    pub matched_pairs: Vec<(f64, f64)>,
    pub consistent_pairs: Vec<(f64, f64)>,
}

impl Default for MatchResult {
    fn default() -> Self {
        Self {
            confidence: 0.0,
            scale_factor: 1.0,
            offset: 0.0,
            matched_pairs: Vec::new(),
            consistent_pairs: Vec::new(),
        }
    }
}

pub fn generate_fingerprints(samples: &[f32], sample_rate: u32) -> AppResult<Vec<Fingerprint>> {
    let mono_samples = if sample_rate != FINGERPRINT_SAMPLE_RATE {
        super::decoder::resample(samples, sample_rate, FINGERPRINT_SAMPLE_RATE)
    } else {
        samples.to_vec()
    };

    let spectrogram = compute_spectrogram(&mono_samples)?;
    let fingerprints = generate_hashes(&spectrogram);

    Ok(fingerprints)
}

fn compute_spectrogram(samples: &[f32]) -> AppResult<Vec<Vec<f32>>> {
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FRAME_SIZE);

    let mut spectrogram = Vec::new();
    let mut window = vec![0.0f32; FRAME_SIZE];
    for i in 0..FRAME_SIZE {
        window[i] = 0.5 * (1.0
            - (2.0 * std::f32::consts::PI * i as f32 / (FRAME_SIZE - 1) as f32).cos());
    }

    let mut start = 0;
    while start + FRAME_SIZE <= samples.len() {
        let mut frame: Vec<Complex<f32>> = samples[start..start + FRAME_SIZE]
            .iter()
            .enumerate()
            .map(|(i, &s)| Complex::new(s * window[i], 0.0))
            .collect();

        fft.process(&mut frame);

        let magnitudes: Vec<f32> = frame
            .iter()
            .take(FRAME_SIZE / 2)
            .map(|c| c.norm_sqr().sqrt())
            .collect();

        spectrogram.push(magnitudes);
        start += HOP_SIZE;
    }

    if spectrogram.is_empty() {
        return Err(AppError::FingerprintError(
            "音频太短，无法生成指纹".to_string(),
        ));
    }

    Ok(spectrogram)
}

fn quantize_dt(dt_frames: usize) -> (u8, usize) {
    for (bucket_idx, &threshold) in DT_BUCKETS.iter().enumerate() {
        if dt_frames <= threshold {
            return (bucket_idx as u8, threshold);
        }
    }
    ((DT_BUCKETS.len() - 1) as u8, DT_BUCKETS[DT_BUCKETS.len() - 1])
}

fn generate_hashes(spectrogram: &[Vec<f32>]) -> Vec<Fingerprint> {
    let num_frames = spectrogram.len();
    let mut peaks: Vec<Vec<usize>> = Vec::with_capacity(num_frames);

    for frame_mags in spectrogram {
        let frame_peaks = find_peaks(frame_mags);
        peaks.push(frame_peaks);
    }

    let mut fingerprints = Vec::new();

    for i in 0..peaks.len() {
        let current_peaks = &peaks[i];

        for j in 1..=FAN_VALUE {
            let future_idx = i + j;
            if future_idx >= peaks.len() {
                break;
            }

            let future_peaks = &peaks[future_idx];
            let (dt_bucket, _) = quantize_dt(j);

            for &peak1 in current_peaks {
                for &peak2 in future_peaks {
                    if peak1 < FINGERPRINT_BINS && peak2 < FINGERPRINT_BINS {
                        let freq_hash = hash_frequencies_only(peak1, peak2);
                        let hash = hash_frequencies_dt(peak1, peak2, dt_bucket as usize);
                        let timestamp = i as f64 * HOP_SIZE as f64
                            / FINGERPRINT_SAMPLE_RATE as f64;

                        fingerprints.push(Fingerprint {
                            hash,
                            freq_hash,
                            timestamp,
                            frame_idx: i,
                            f1: peak1 as u16,
                            f2: peak2 as u16,
                            dt_bucket,
                        });
                    }
                }
            }
        }
    }

    fingerprints
}

fn find_peaks(magnitudes: &[f32]) -> Vec<usize> {
    let mut peaks = Vec::new();
    let threshold = compute_threshold(magnitudes);
    let min_bins_between_peaks = 2;

    let mut last_peak_idx = -100i32;

    for i in MIN_FREQ_BIN..=MAX_FREQ_BIN.min(magnitudes.len() - 1) {
        if magnitudes[i] < threshold {
            continue;
        }

        let is_peak = if i == 0 {
            magnitudes[i] > magnitudes[i + 1]
        } else if i == magnitudes.len() - 1 {
            magnitudes[i] > magnitudes[i - 1]
        } else {
            magnitudes[i] > magnitudes[i - 1] && magnitudes[i] > magnitudes[i + 1]
        };

        if is_peak && (i as i32 - last_peak_idx) >= min_bins_between_peaks {
            peaks.push(i);
            last_peak_idx = i as i32;
        }
    }

    if peaks.len() > 15 {
        peaks.truncate(15);
    }

    peaks
}

fn compute_threshold(magnitudes: &[f32]) -> f32 {
    let start = MIN_FREQ_BIN;
    let end = MAX_FREQ_BIN.min(magnitudes.len() - 1);
    let count = (end - start + 1) as f32;

    let sum: f32 = magnitudes[start..=end].iter().sum();
    let mean = sum / count;

    let variance: f32 = magnitudes[start..=end]
        .iter()
        .map(|&v| (v - mean).powi(2))
        .sum::<f32>()
        / count;
    let std = variance.sqrt();

    mean + std * 0.3
}

fn hash_frequencies_only(f1: usize, f2: usize) -> u64 {
    let f1 = f1 as u64;
    let f2 = f2 as u64;
    (f1 << 32) | f2
}

fn hash_frequencies_dt(f1: usize, f2: usize, dt_bucket: usize) -> u64 {
    let f1 = f1 as u64;
    let f2 = f2 as u64;
    let dt = dt_bucket as u64;
    (f1 << 40) | (f2 << 24) | (dt & 0xFF)
}

pub fn extract_frequencies_from_hash(hash: u64) -> (usize, usize, usize) {
    let f1 = ((hash >> 40) & 0xFFFF) as usize;
    let f2 = ((hash >> 24) & 0xFFFF) as usize;
    let dt = (hash & 0xFF) as usize;
    (f1, f2, dt)
}

pub fn match_fingerprints(
    query_fingerprints: &[Fingerprint],
    target_fingerprints: &[Fingerprint],
) -> (f64, Vec<(f64, f64)>) {
    let result = match_fingerprints_robust(query_fingerprints, target_fingerprints);
    (result.confidence, result.consistent_pairs)
}

pub fn match_fingerprints_robust(
    query_fingerprints: &[Fingerprint],
    target_fingerprints: &[Fingerprint],
) -> MatchResult {
    if query_fingerprints.is_empty() || target_fingerprints.is_empty() {
        return MatchResult::default();
    }

    let freq_index = build_freq_index(target_fingerprints);

    let mut raw_matches: Vec<(f64, f64)> = Vec::new();
    for qfp in query_fingerprints {
        if let Some(target_entries) = freq_index.get(&qfp.freq_hash) {
            for &(t_time, _t_frame) in target_entries {
                raw_matches.push((qfp.timestamp, t_time));
            }
        }
    }

    if raw_matches.len() < 5 {
        return MatchResult {
            matched_pairs: raw_matches,
            ..MatchResult::default()
        };
    }

    let mut best_result = MatchResult::default();

    for &scale in SCALE_FACTORS {
        let scaled_matches: Vec<(f64, f64)> = raw_matches
            .iter()
            .map(|&(q, t)| (q * scale, t))
            .collect();

        let (consistent, offset, inlier_count) =
            find_consistent_matches(&scaled_matches, 0.3);

        let total_scaled = scaled_matches.len() as f64;
        let consistency_score = inlier_count as f64 / total_scaled.max(1.0);

        let query_coverage = {
            if consistent.len() >= 2 {
                let min_q = consistent.iter().map(|(q, _)| *q).fold(f64::INFINITY, f64::min);
                let max_q = consistent.iter().map(|(q, _)| *q).fold(f64::NEG_INFINITY, f64::max);
                let query_duration = query_fingerprints
                    .last()
                    .map(|fp| fp.timestamp)
                    .unwrap_or(1.0);
                ((max_q - min_q) / query_duration.max(0.1)).min(1.0)
            } else {
                0.0
            }
        };

        let density_score = if consistent.len() >= 2 {
            let min_q = consistent.iter().map(|(q, _)| *q).fold(f64::INFINITY, f64::min);
            let max_q = consistent.iter().map(|(q, _)| *q).fold(f64::NEG_INFINITY, f64::max);
            let duration = (max_q - min_q).max(0.1);
            (consistent.len() as f64 / duration).min(50.0) / 50.0
        } else {
            0.0
        };

        let confidence = (consistency_score * 0.4 + query_coverage * 0.35 + density_score * 0.25)
            .min(1.0);

        if confidence > best_result.confidence {
            let original_pairs: Vec<(f64, f64)> = consistent
                .iter()
                .map(|&(q_scaled, t)| (q_scaled / scale, t))
                .collect();

            best_result = MatchResult {
                confidence,
                scale_factor: scale,
                offset,
                matched_pairs: raw_matches.clone(),
                consistent_pairs: original_pairs,
            };
        }
    }

    if best_result.confidence < 0.05 {
        best_result = dtw_enhanced_match(query_fingerprints, target_fingerprints, &best_result);
    }

    best_result
}

fn build_freq_index(fingerprints: &[Fingerprint]) -> HashMap<u64, Vec<(f64, usize)>> {
    let mut index: HashMap<u64, Vec<(f64, usize)>> = HashMap::new();
    for fp in fingerprints {
        index
            .entry(fp.freq_hash)
            .or_default()
            .push((fp.timestamp, fp.frame_idx));
    }
    index
}

fn find_consistent_matches(
    matches: &[(f64, f64)],
    tolerance: f64,
) -> (Vec<(f64, f64)>, f64, usize) {
    if matches.is_empty() {
        return (Vec::new(), 0.0, 0);
    }

    let mut sorted = matches.to_vec();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let offset_histogram = histogram_offsets(&sorted, tolerance);
    let (best_offset, peak_count) = offset_histogram
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .unwrap_or((0.0, 0));

    let inliers: Vec<(f64, f64)> = sorted
        .iter()
        .filter(|&&(q, t)| {
            let offset = t - q;
            (offset - best_offset).abs() < tolerance
        })
        .copied()
        .collect();

    let refined = lis_filter(&inliers);

    (refined.clone(), best_offset, refined.len().max(peak_count))
}

fn histogram_offsets(matches: &[(f64, f64)], tolerance: f64) -> Vec<(f64, usize)> {
    let bin_width = tolerance / 2.0;
    let mut bins: HashMap<i64, usize> = HashMap::new();

    for &(q, t) in matches {
        let offset = t - q;
        let bin = (offset / bin_width).round() as i64;
        *bins.entry(bin).or_insert(0) += 1;
    }

    bins.into_iter()
        .map(|(bin, count)| (bin as f64 * bin_width, count))
        .collect()
}

fn lis_filter(matches: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if matches.len() < 3 {
        return matches.to_vec();
    }

    let mut sorted = matches.to_vec();
    sorted.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });

    let n = sorted.len();
    let mut dp = vec![1i32; n];
    let mut prev: Vec<isize> = vec![-1; n];

    for i in 1..n {
        for j in 0..i {
            if sorted[j].1 <= sorted[i].1 && sorted[j].0 <= sorted[i].0 {
                let slope_ij = if (sorted[i].0 - sorted[j].0).abs() > 1e-6 {
                    (sorted[i].1 - sorted[j].1) / (sorted[i].0 - sorted[j].0)
                } else {
                    1.0
                };
                if slope_ij > 0.3 && slope_ij < 3.0 {
                    if dp[j] + 1 > dp[i] {
                        dp[i] = dp[j] + 1;
                        prev[i] = j as isize;
                    }
                }
            }
        }
    }

    let mut best_idx = 0;
    for i in 1..n {
        if dp[i] > dp[best_idx] {
            best_idx = i;
        }
    }

    let mut sequence = Vec::new();
    let mut idx = best_idx as isize;
    while idx >= 0 {
        sequence.push(sorted[idx as usize]);
        idx = prev[idx as usize];
    }
    sequence.reverse();

    if sequence.len() < 3 {
        return sorted;
    }

    sequence
}

fn dtw_enhanced_match(
    query_fps: &[Fingerprint],
    target_fps: &[Fingerprint],
    initial: &MatchResult,
) -> MatchResult {
    let mut result = initial.clone();

    let target_frames: HashMap<u64, Vec<usize>> = {
        let mut m: HashMap<u64, Vec<usize>> = HashMap::new();
        for fp in target_fps {
            m.entry(fp.freq_hash).or_default().push(fp.frame_idx);
        }
        m
    };

    let mut cost_matrix: Vec<(usize, usize, f64)> = Vec::new();
    for (qi, qfp) in query_fps.iter().enumerate() {
        if let Some(target_frame_list) = target_frames.get(&qfp.freq_hash) {
            for &ti in target_frame_list {
                cost_matrix.push((qi, ti, 0.0));
            }
        }
    }

    if cost_matrix.len() < 10 {
        return result;
    }

    cost_matrix.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let n = cost_matrix.len();
    let mut dp_len = vec![1i32; n];
    let mut dp_score = vec![0.0f64; n];

    for i in 0..n {
        let (qi, ti, _) = cost_matrix[i];
        for j in 0..i {
            let (qj, tj, _) = cost_matrix[j];
            if qj < qi && tj < ti {
                let dq = (qi as i32 - qj as i32) as f64;
                let dt = (ti as i32 - tj as i32) as f64;
                let ratio = dt / dq.max(0.1);
                if ratio > 0.5 && ratio < 2.0 {
                    let path_cost = 1.0 - (ratio - 1.0).abs().min(0.5);
                    if dp_len[j] + 1 > dp_len[i] {
                        dp_len[i] = dp_len[j] + 1;
                        dp_score[i] = dp_score[j] + path_cost;
                    }
                }
            }
        }
    }

    let max_len = *dp_len.iter().max().unwrap_or(&0) as f64;
    let total_score = *dp_score.iter().max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(&0.0);

    let potential = (total_score / 50.0).min(1.0) * 0.6
        + (max_len / 100.0).min(1.0) * 0.4;

    if potential > result.confidence {
        let mut matched: Vec<(f64, f64)> = Vec::new();
        for (qi, ti, _) in &cost_matrix {
            if qi < &query_fps.len() {
                if let Some(qfp) = query_fps.get(*qi) {
                    let t_time = if let Some(tfp) = target_fps.iter().find(|t| t.frame_idx == *ti) {
                        tfp.timestamp
                    } else {
                        (*ti as f64) * HOP_SIZE as f64 / FINGERPRINT_SAMPLE_RATE as f64
                    };
                    matched.push((qfp.timestamp, t_time));
                }
            }
        }
        result.confidence = potential;
        result.consistent_pairs = matched;
    }

    result
}

#[derive(Debug, Clone)]
pub struct SegmentMatch {
    pub query_start: f64,
    pub query_end: f64,
    pub target_start: f64,
    pub target_end: f64,
    pub confidence: f64,
    pub scale_factor: f64,
}

pub fn extract_segments(
    matched_pairs: &[(f64, f64)],
    scale_factor: f64,
) -> Vec<SegmentMatch> {
    if matched_pairs.is_empty() {
        return Vec::new();
    }

    let mut pairs = matched_pairs.to_vec();
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut segments = Vec::new();
    let time_tolerance_q = 0.5;
    let time_tolerance_t = 0.5 * scale_factor;
    let min_segment_length = 0.5;

    let mut current: Option<(f64, f64, f64, f64, Vec<(f64, f64)>)> = None;

    for &(q_t, t_t) in &pairs {
        match &mut current {
            Some((q_s, q_e, t_s, t_e, pts)) => {
                let expected_t = *t_s + (q_t - *q_s) * scale_factor;
                let q_gap = q_t - *q_e;
                let slope_ok = if q_gap > 0.01 {
                    let slope = (t_t - *t_e) / q_gap;
                    (slope - scale_factor).abs() < 0.5 * scale_factor
                } else {
                    true
                };
                let time_match = (t_t - expected_t).abs() < time_tolerance_t;

                if q_gap < time_tolerance_q && (time_match || slope_ok) {
                    *q_e = q_t;
                    *t_e = t_t;
                    pts.push((q_t, t_t));
                } else {
                    let duration = *q_e - *q_s;
                    if duration >= min_segment_length {
                        let conf = pts.len() as f64 / (duration * 20.0).max(1.0);
                        segments.push(SegmentMatch {
                            query_start: *q_s,
                            query_end: *q_e,
                            target_start: *t_s,
                            target_end: *t_e,
                            confidence: conf.min(1.0),
                            scale_factor,
                        });
                    }
                    current = Some((q_t, q_t, t_t, t_t, vec![(q_t, t_t)]));
                }
            }
            None => {
                current = Some((q_t, q_t, t_t, t_t, vec![(q_t, t_t)]));
            }
        }
    }

    if let Some((q_s, q_e, t_s, t_e, pts)) = current {
        let duration = q_e - q_s;
        if duration >= min_segment_length {
            let conf = pts.len() as f64 / (duration * 20.0).max(1.0);
            segments.push(SegmentMatch {
                query_start: q_s,
                query_end: q_e,
                target_start: t_s,
                target_end: t_e,
                confidence: conf.min(1.0),
                scale_factor,
            });
        }
    }

    segments
}

pub fn sliding_window_match(
    query_fps: &[Fingerprint],
    target_fps: &[Fingerprint],
) -> MatchResult {
    if query_fps.len() < 20 {
        return match_fingerprints_robust(query_fps, target_fps);
    }

    let query_duration = query_fps.last().map(|fp| fp.timestamp).unwrap_or(0.0);
    let window_size = (query_duration / 3.0).max(3.0).min(15.0);
    let step_size = window_size / 2.0;

    let mut best = MatchResult::default();
    let mut window_start = 0.0f64;

    while window_start < query_duration {
        let window_end = window_start + window_size;
        let window_fps: Vec<Fingerprint> = query_fps
            .iter()
            .filter(|fp| fp.timestamp >= window_start && fp.timestamp < window_end)
            .cloned()
            .collect();

        if window_fps.len() >= 10 {
            let result = match_fingerprints_robust(&window_fps, target_fps);
            let window_boost = (window_fps.len() as f64 / query_fps.len() as f64).sqrt();
            let adjusted_conf = (result.confidence * window_boost * 1.1).min(1.0);

            if adjusted_conf > best.confidence {
                let mut shifted_pairs = result.consistent_pairs.clone();
                for (q, _) in &mut shifted_pairs {
                    *q += window_start;
                }

                best = MatchResult {
                    confidence: adjusted_conf,
                    scale_factor: result.scale_factor,
                    offset: result.offset,
                    matched_pairs: result.matched_pairs,
                    consistent_pairs: shifted_pairs,
                };
            }
        }

        window_start += step_size;
    }

    let full_result = match_fingerprints_robust(query_fps, target_fps);
    if full_result.confidence > best.confidence {
        full_result
    } else {
        best
    }
}
