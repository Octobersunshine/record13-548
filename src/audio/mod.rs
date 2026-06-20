pub mod decoder;
pub mod fingerprint;

pub use decoder::{decode_audio_file, to_mono, DecodedAudio};
pub use fingerprint::{
    generate_fingerprints, match_fingerprints, Fingerprint, FINGERPRINT_SAMPLE_RATE,
};
