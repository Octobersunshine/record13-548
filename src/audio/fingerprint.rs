use crate::errors::{AppError, AppResult};
use rustfft::{FftPlanner, num_complex::Complex};

pub const FINGERPRINT_SAMPLE_RATE: u32 = 22050;
pub const FRAME_SIZE: usize = 2048;
pub const HOP_SIZE: usize = 512;
pub const FINGERPRINT_BINS: usize = 64;
pub const FAN_VALUE: usize = 5;
pub const MIN_FREQ_BIN: usize = 0;
pub const MAX_FREQ_BIN: usize = 63;

#[derive(Debug, Clone)]
pub struct Fingerprint {
    pub hash: u64,
    pub timestamp: f64,
}

pub fn generate_fingerprints(samples: &[f32], sample_rate: u32) -> AppResult<Vec<Fingerprint>> {
    let mono_samples = if sample_rate != FINGERPRINT_SAMPLE_RATE {
        super::decoder::resample(samples, sample_rate, FINGERPRINT_SAMPLE_RATE)
    } else {
        samples.to_vec()
    };

    let spectrogram = compute_spectrogram(&mono_samples)?;
    let fingerprints = generate_hashes(&spectrogram, FINGERPRINT_SAMPLE_RATE as f64);

    Ok(fingerprints)
}

fn compute_spectrogram(samples: &[f32]) -> AppResult<Vec<Vec<f32>>> {
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FRAME_SIZE);

    let mut spectrogram = Vec::new();
    let mut window = vec![0.0f32; FRAME_SIZE];
    for i in 0..FRAME_SIZE {
        window[i] = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FRAME_SIZE - 1) as f32).cos());
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
        return Err(AppError::FingerprintError("音频太短，无法生成指纹".to_string()));
    }

    Ok(spectrogram)
}

fn generate_hashes(spectrogram: &[Vec<f32>], _sample_rate: f64) -> Vec<Fingerprint> {
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

            for &peak1 in current_peaks {
                for &peak2 in future_peaks {
                    if peak1 < FINGERPRINT_BINS && peak2 < FINGERPRINT_BINS {
                        let hash = hash_frequencies(peak1, peak2, j);
                        let timestamp = i as f64 * HOP_SIZE as f64 / FINGERPRINT_SAMPLE_RATE as f64;

                        fingerprints.push(Fingerprint { hash, timestamp });
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
    let min_bins_between_peaks = 3;

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

    if peaks.len() > 10 {
        peaks.truncate(10);
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

    mean + std * 0.5
}

fn hash_frequencies(f1: usize, f2: usize, delta_t: usize) -> u64 {
    let f1 = f1 as u64;
    let f2 = f2 as u64;
    let dt = delta_t as u64;

    (f1 << 40) | (f2 << 24) | (dt & 0xFFFF)
}

pub fn extract_frequencies_from_hash(hash: u64) -> (usize, usize, usize) {
    let f1 = ((hash >> 40) & 0xFFFF) as usize;
    let f2 = ((hash >> 24) & 0xFFFF) as usize;
    let dt = (hash & 0xFFFF) as usize;
    (f1, f2, dt)
}

pub fn match_fingerprints(
    query_fingerprints: &[Fingerprint],
    target_fingerprints: &[Fingerprint],
) -> (f64, Vec<(f64, f64)>) {
    use std::collections::HashMap;

    let mut target_hash_map: HashMap<u64, Vec<f64>> = HashMap::new();
    for fp in target_fingerprints {
        target_hash_map.entry(fp.hash).or_default().push(fp.timestamp);
    }

    let mut time_differences: Vec<f64> = Vec::new();
    let mut matched_pairs: Vec<(f64, f64)> = Vec::new();

    for query_fp in query_fingerprints {
        if let Some(target_times) = target_hash_map.get(&query_fp.hash) {
            for &target_time in target_times {
                let diff = target_time - query_fp.timestamp;
                time_differences.push(diff);
                matched_pairs.push((query_fp.timestamp, target_time));
            }
        }
    }

    if time_differences.is_empty() {
        return (0.0, Vec::new());
    }

    let bin_width = 0.1;
    let mut hist: HashMap<i64, usize> = HashMap::new();

    for &diff in &time_differences {
        let bin = (diff / bin_width).round() as i64;
        *hist.entry(bin).or_insert(0) += 1;
    }

    let max_count = *hist.values().max().unwrap_or(&0);
    let total_matches = time_differences.len();
    let confidence = if total_matches > 0 {
        max_count as f64 / query_fingerprints.len().min(100) as f64
    } else {
        0.0
    };

    (confidence.min(1.0), matched_pairs)
}
