pub mod decoder;
pub mod fingerprint;

pub use decoder::{decode_audio_file, to_mono, DecodedAudio};
pub use fingerprint::{
    extract_segments, generate_fingerprints, match_fingerprints, sliding_window_match,
    Fingerprint, MatchResult, SegmentMatch, FINGERPRINT_SAMPLE_RATE,
};
