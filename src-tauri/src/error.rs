use std::path::PathBuf;

/// Path to the temp WAV used for a single transcription.
pub fn temp_wav_path() -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("hotyap_rec_{ts}.wav"))
}
