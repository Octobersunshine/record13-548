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
    pub raw_query_start: f64,
    pub raw_query_end: f64,
    pub query_start: f64,
    pub query_end: f64,
    pub raw_target_start: f64,
    pub raw_target_end: f64,
    pub target_start: f64,
    pub target_end: f64,
    pub confidence: f64,
    pub boundary_confidence: f64,
    pub scale_factor: f64,
    pub matched_points_count: usize,
    pub matched_density: f64,
    pub regression_slope: f64,
    pub regression_intercept: f64,
    pub regression_r2: f64,
}

struct LinearRegressionResult {
    slope: f64,
    intercept: f64,
    r2: f64,
}

fn linear_regression(points: &[(f64, f64)]) -> LinearRegressionResult {
    let n = points.len() as f64;
    if n < 2.0 {
        return LinearRegressionResult {
            slope: 1.0,
            intercept: 0.0,
            r2: 0.0,
        };
    }

    let sum_x: f64 = points.iter().map(|(x, _)| *x).sum();
    let sum_y: f64 = points.iter().map(|(_, y)| *y).sum();
    let sum_xy: f64 = points.iter().map(|(x, y)| *x * *y).sum();
    let sum_x2: f64 = points.iter().map(|(x, _)| *x * *x).sum();
    let _sum_y2: f64 = points.iter().map(|(_, y)| *y * *y).sum();

    let denom = n * sum_x2 - sum_x * sum_x;
    let slope = if denom.abs() > 1e-10 {
        (n * sum_xy - sum_x * sum_y) / denom
    } else {
        1.0
    };
    let intercept = (sum_y - slope * sum_x) / n;

    let y_mean = sum_y / n;
    let ss_tot: f64 = points.iter().map(|(_, y)| (*y - y_mean).powi(2)).sum();
    let ss_res: f64 = points
        .iter()
        .map(|(x, y)| (*y - (slope * *x + intercept)).powi(2))
        .sum();

    let r2 = if ss_tot.abs() > 1e-10 {
        1.0 - ss_res / ss_tot
    } else {
        0.0
    };

    LinearRegressionResult {
        slope,
        intercept,
        r2: r2.max(0.0).min(1.0),
    }
}

fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn refine_segment_boundaries(
    points: &[(f64, f64)],
    regression: &LinearRegressionResult,
    raw_q_start: f64,
    raw_q_end: f64,
    raw_t_start: f64,
    raw_t_end: f64,
) -> (f64, f64, f64, f64, f64) {
    let num_points = points.len();
    if num_points < 4 {
        let boundary_conf = (num_points as f64 / 10.0).min(0.5);
        return (
            raw_q_start,
            raw_q_end,
            raw_t_start,
            raw_t_end,
            boundary_conf,
        );
    }

    let residuals: Vec<(f64, f64, f64)> = points
        .iter()
        .map(|&(x, y)| {
            let predicted = regression.slope * x + regression.intercept;
            let residual = (y - predicted).abs();
            (x, y, residual)
        })
        .collect();

    let p75_residual = percentile(
        &residuals.iter().map(|&(_, _, r)| r).collect::<Vec<_>>(),
        0.75,
    );
    let inlier_threshold = p75_residual * 2.0 + 0.05;

    let inliers: Vec<(f64, f64)> = residuals
        .iter()
        .filter(|&&(_, _, r)| r <= inlier_threshold)
        .map(|&(x, y, _)| (x, y))
        .collect();

    let inlier_ratio = inliers.len() as f64 / num_points as f64;

    if inliers.len() < 3 {
        let boundary_conf = (inlier_ratio * 0.6).min(0.7);
        return (
            raw_q_start,
            raw_q_end,
            raw_t_start,
            raw_t_end,
            boundary_conf,
        );
    }

    let q_times: Vec<f64> = inliers.iter().map(|&(q, _)| q).collect();
    let t_times: Vec<f64> = inliers.iter().map(|&(_, t)| t).collect();

    let q_p05 = percentile(&q_times, 0.05);
    let q_p95 = percentile(&q_times, 0.95);
    let t_p05 = percentile(&t_times, 0.05);
    let t_p95 = percentile(&t_times, 0.95);

    let refined_q_start = q_p05;
    let refined_q_end = q_p95;
    let refined_t_start = t_p05;
    let refined_t_end = t_p95;

    let regression_quality = regression.r2.max(0.0).min(1.0);
    let density_quality = (num_points as f64
        / ((raw_q_end - raw_q_start) * 30.0).max(1.0))
    .min(1.0);
    let boundary_confidence =
        (regression_quality * 0.5 + inlier_ratio * 0.3 + density_quality * 0.2)
            .min(1.0);

    (
        refined_q_start.max(0.0),
        refined_q_end,
        refined_t_start.max(0.0),
        refined_t_end,
        boundary_confidence,
    )
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
    let min_points_per_segment = 5;

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
                    if duration >= min_segment_length && pts.len() >= min_points_per_segment {
                        if let Some(seg) = build_segment(
                            pts,
                            *q_s,
                            *q_e,
                            *t_s,
                            *t_e,
                            scale_factor,
                        ) {
                            segments.push(seg);
                        }
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
        if duration >= min_segment_length && pts.len() >= min_points_per_segment {
            if let Some(seg) =
                build_segment(&pts, q_s, q_e, t_s, t_e, scale_factor)
            {
                segments.push(seg);
            }
        }
    }

    merge_overlapping_segments(&mut segments, scale_factor);

    segments
}

fn build_segment(
    points: &[(f64, f64)],
    raw_q_s: f64,
    raw_q_e: f64,
    raw_t_s: f64,
    raw_t_e: f64,
    scale_factor: f64,
) -> Option<SegmentMatch> {
    if points.len() < 3 {
        return None;
    }

    let regression = linear_regression(points);

    let (ref_q_s, ref_q_e, ref_t_s, ref_t_e, boundary_conf) = refine_segment_boundaries(
        points,
        &regression,
        raw_q_s,
        raw_q_e,
        raw_t_s,
        raw_t_e,
    );

    let query_duration = ref_q_e - ref_q_s;
    let base_conf = points.len() as f64 / (query_duration * 25.0).max(1.0);
    let confidence =
        (base_conf * 0.5 + regression.r2 * 0.3 + boundary_conf * 0.2).min(1.0);

    let matched_density = if query_duration > 0.0 {
        points.len() as f64 / query_duration
    } else {
        0.0
    };

    Some(SegmentMatch {
        raw_query_start: raw_q_s,
        raw_query_end: raw_q_e,
        query_start: ref_q_s,
        query_end: ref_q_e,
        raw_target_start: raw_t_s,
        raw_target_end: raw_t_e,
        target_start: ref_t_s,
        target_end: ref_t_e,
        confidence,
        boundary_confidence: boundary_conf,
        scale_factor,
        matched_points_count: points.len(),
        matched_density,
        regression_slope: regression.slope,
        regression_intercept: regression.intercept,
        regression_r2: regression.r2,
    })
}

fn merge_overlapping_segments(segments: &mut Vec<SegmentMatch>, scale_factor: f64) {
    if segments.len() < 2 {
        return;
    }

    segments.sort_by(|a, b| {
        a.query_start
            .partial_cmp(&b.query_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let gap_tolerance = 1.0;
    let mut merged: Vec<SegmentMatch> = Vec::new();
    let mut i = 0;

    while i < segments.len() {
        let mut current = segments[i].clone();
        let mut j = i + 1;

        while j < segments.len() {
            let next = &segments[j];
            let query_gap = next.query_start - current.query_end;
            let target_gap = next.target_start - current.target_end;

            let can_merge = query_gap < gap_tolerance
                && target_gap < gap_tolerance * scale_factor
                && (next.scale_factor - current.scale_factor).abs() < 0.15;

            if can_merge {
                let all_points_dummy =
                    current.matched_points_count + next.matched_points_count;
                let merged_r2 = (current.regression_r2 * current.matched_points_count as f64
                    + next.regression_r2 * next.matched_points_count as f64)
                    / all_points_dummy as f64;

                current = SegmentMatch {
                    raw_query_start: current.raw_query_start.min(next.raw_query_start),
                    raw_query_end: current.raw_query_end.max(next.raw_query_end),
                    query_start: current.query_start.min(next.query_start),
                    query_end: current.query_end.max(next.query_end),
                    raw_target_start: current.raw_target_start.min(next.raw_target_start),
                    raw_target_end: current.raw_target_end.max(next.raw_target_end),
                    target_start: current.target_start.min(next.target_start),
                    target_end: current.target_end.max(next.target_end),
                    confidence: (current.confidence + next.confidence) / 2.0,
                    boundary_confidence:
                        (current.boundary_confidence + next.boundary_confidence) / 2.0,
                    scale_factor: (current.scale_factor + next.scale_factor) / 2.0,
                    matched_points_count: all_points_dummy,
                    matched_density: all_points_dummy as f64
                        / (current.query_end - current.query_start).max(0.1),
                    regression_slope: (current.regression_slope + next.regression_slope) / 2.0,
                    regression_intercept: (current.regression_intercept
                        + next.regression_intercept)
                        / 2.0,
                    regression_r2: merged_r2,
                };
                j += 1;
            } else {
                break;
            }
        }

        merged.push(current);
        i = j;
    }

    *segments = merged;
}

pub fn build_timeline_summary(
    segments: &[SegmentMatch],
    query_total_duration: f64,
    _matched_track_duration: f64,
) -> (f64, f64, f64, Vec<(f64, f64)>) {
    if segments.is_empty() {
        return (0.0, 0.0, 1.0, Vec::new());
    }

    let mut ranges: Vec<(f64, f64)> = segments
        .iter()
        .map(|s| (s.query_start, s.query_end))
        .collect();

    ranges.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut merged: Vec<(f64, f64)> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut() {
            if range.0 <= last.1 + 0.3 {
                last.1 = last.1.max(range.1);
            } else {
                merged.push(range);
            }
        } else {
            merged.push(range);
        }
    }

    let total_seconds: f64 = merged.iter().map(|(s, e)| e - s).sum();
    let ratio = if query_total_duration > 0.0 {
        total_seconds / query_total_duration
    } else {
        0.0
    };

    let max_conf = segments
        .iter()
        .map(|s| s.confidence)
        .fold(0.0_f64, f64::max);

    let _dominant_scale = {
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

    (total_seconds, ratio, max_conf, merged)
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
