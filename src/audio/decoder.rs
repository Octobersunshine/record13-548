use crate::errors::{AppError, AppResult};
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration: f64,
}

pub fn decode_audio_file(path: &Path) -> AppResult<DecodedAudio> {
    let file = std::fs::File::open(path).map_err(|e| AppError::FileError(e.to_string()))?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let format_opts = FormatOptions::default();
    let decoder_opts = DecoderOptions::default();
    let metadata_opts = MetadataOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| AppError::AudioDecodeError(format!("无法识别音频格式: {}", e)))?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| AppError::AudioDecodeError("未找到默认音频轨道".to_string()))?;

    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .map_err(|e| AppError::AudioDecodeError(format!("无法创建解码器: {}", e)))?;

    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| AppError::AudioDecodeError("未知采样率".to_string()))?;

    let channels = track
        .codec_params
        .channels
        .ok_or_else(|| AppError::AudioDecodeError("未知声道数".to_string()))?
        .count() as u16;

    let time_base = track.codec_params.time_base;
    let n_frames = track.codec_params.n_frames;

    let duration = match (time_base, n_frames) {
        (Some(tb), Some(nf)) => tb.calc_time(nf).seconds as f64 + tb.calc_time(nf).frac,
        _ => 0.0,
    };

    let mut samples: Vec<f32> = Vec::new();
    let mut sample_buffer: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(symphonia::core::errors::Error::ResetRequired) => continue,
            Err(e) => {
                return Err(AppError::AudioDecodeError(format!(
                    "读取数据包失败: {}",
                    e
                )));
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder
            .decode(&packet)
            .map_err(|e| AppError::AudioDecodeError(format!("解码失败: {}", e)))?;

        if sample_buffer.is_none() {
            let spec = *decoded.spec();
            let duration = decoded.capacity() as u64;
            sample_buffer = Some(SampleBuffer::new(duration, spec));
        }

        if let Some(ref mut buf) = sample_buffer {
            buf.copy_interleaved_ref(decoded);
            samples.extend_from_slice(buf.samples());
        }
    }

    Ok(DecodedAudio {
        samples,
        sample_rate,
        channels,
        duration,
    })
}

pub fn to_mono(audio: &DecodedAudio) -> Vec<f32> {
    if audio.channels == 1 {
        return audio.samples.clone();
    }

    let channels = audio.channels as usize;
    let mut mono = Vec::with_capacity(audio.samples.len() / channels);

    for chunk in audio.samples.chunks(channels) {
        let sum: f32 = chunk.iter().sum();
        mono.push(sum / channels as f32);
    }

    mono
}

pub fn resample(samples: &[f32], original_rate: u32, target_rate: u32) -> Vec<f32> {
    if original_rate == target_rate {
        return samples.to_vec();
    }

    let ratio = target_rate as f64 / original_rate as f64;
    let new_len = (samples.len() as f64 * ratio) as usize;
    let mut resampled = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let pos = i as f64 / ratio;
        let idx = pos.floor() as usize;
        let frac = pos.fract() as f32;

        if idx + 1 < samples.len() {
            let sample = samples[idx] * (1.0 - frac) + samples[idx + 1] * frac;
            resampled.push(sample);
        } else if idx < samples.len() {
            resampled.push(samples[idx]);
        }
    }

    resampled
}
