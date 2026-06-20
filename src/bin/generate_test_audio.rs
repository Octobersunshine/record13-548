use hound;
use std::path::Path;

fn generate_sine_wave(freq: f32, sample_rate: u32, duration: f32, amplitude: f32) -> Vec<f32> {
    let num_samples = (sample_rate as f32 * duration) as usize;
    let mut samples = Vec::with_capacity(num_samples);
    
    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let sample = amplitude * (2.0 * std::f32::consts::PI * freq * t).sin();
        samples.push(sample);
    }
    
    samples
}

fn generate_melody(sample_rate: u32) -> Vec<f32> {
    let mut samples = Vec::new();
    let note_duration = 0.25;
    let amplitude = 0.5;
    
    let notes = [261.63, 293.66, 329.63, 349.23, 392.00, 440.00, 493.88, 523.25];
    
    for &freq in &notes {
        let note_samples = generate_sine_wave(freq, sample_rate, note_duration, amplitude);
        samples.extend_from_slice(&note_samples);
    }
    
    for &freq in notes.iter().rev() {
        let note_samples = generate_sine_wave(freq, sample_rate, note_duration, amplitude);
        samples.extend_from_slice(&note_samples);
    }
    
    samples
}

fn generate_noise(sample_rate: u32, duration: f32, amplitude: f32) -> Vec<f32> {
    let num_samples = (sample_rate as f32 * duration) as usize;
    let mut samples = Vec::with_capacity(num_samples);
    
    let mut seed = 12345u32;
    for _ in 0..num_samples {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let rnd = (seed >> 16) as f32 / 32768.0 - 1.0;
        samples.push(rnd * amplitude);
    }
    
    samples
}

fn save_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<(), Box<dyn std::error::Error>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    
    let mut writer = hound::WavWriter::create(path, spec)?;
    
    for &sample in samples {
        writer.write_sample(sample)?;
    }
    
    writer.finalize()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sample_rate = 44100;
    let output_dir = Path::new("test_audio");
    
    std::fs::create_dir_all(output_dir)?;
    
    println!("生成测试音频文件...");
    
    let melody_samples = generate_melody(sample_rate);
    let melody_path = output_dir.join("test_track_1.wav");
    save_wav(&melody_path, &melody_samples, sample_rate)?;
    println!("  生成: {} ({}秒)", melody_path.display(), melody_samples.len() as f32 / sample_rate as f32);
    
    let short_samples = generate_sine_wave(440.0, sample_rate, 2.0, 0.8);
    let short_path = output_dir.join("test_short.wav");
    save_wav(&short_path, &short_samples, sample_rate)?;
    println!("  生成: {} ({}秒)", short_path.display(), short_samples.len() as f32 / sample_rate as f32);
    
    let mut mixed_samples = Vec::new();
    let noise = generate_noise(sample_rate, 1.0, 0.1);
    mixed_samples.extend_from_slice(&noise);
    
    let melody_portion: Vec<f32> = melody_samples
        .iter()
        .take((sample_rate as f32 * 2.0) as usize)
        .map(|&s| s * 0.5)
        .collect();
    mixed_samples.extend_from_slice(&melody_portion);
    
    let noise2 = generate_noise(sample_rate, 1.0, 0.1);
    mixed_samples.extend_from_slice(&noise2);
    
    let mixed_path = output_dir.join("test_mixed.wav");
    save_wav(&mixed_path, &mixed_samples, sample_rate)?;
    println!("  生成: {} ({}秒)", mixed_path.display(), mixed_samples.len() as f32 / sample_rate as f32);
    
    let different_samples = generate_sine_wave(880.0, sample_rate, 3.0, 0.6);
    let different_path = output_dir.join("test_different.wav");
    save_wav(&different_path, &different_samples, sample_rate)?;
    println!("  生成: {} ({}秒)", different_path.display(), different_samples.len() as f32 / sample_rate as f32);
    
    println!("\n测试音频生成完成！");
    
    Ok(())
}
