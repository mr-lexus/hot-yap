use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_RECORDING: AtomicU64 = AtomicU64::new(0);

/// Path to the temp WAV used for a single transcription.
pub fn temp_wav_path() -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let sequence = NEXT_RECORDING.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "hotyap_rec_{}_{}_{}.wav",
        std::process::id(),
        ts,
        sequence
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_paths_are_unique() {
        assert_ne!(temp_wav_path(), temp_wav_path());
    }
}
