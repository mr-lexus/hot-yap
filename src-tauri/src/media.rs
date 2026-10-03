use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use serde::Serialize;

const MAX_MEDIA_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const CHUNK_SECONDS: u64 = 10 * 60;
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "aac", "flac", "m4a", "m4v", "mkv", "mov", "mp3", "mp4", "oga", "ogg", "opus", "wav", "webm",
    "wma",
];

#[derive(Clone, Debug, Serialize)]
pub struct MediaFileInfo {
    pub name: String,
    pub extension: String,
    pub size_bytes: u64,
    pub is_video: bool,
}

pub fn inspect(path: &str) -> Result<(PathBuf, MediaFileInfo), String> {
    let path =
        fs::canonicalize(path).map_err(|error| format!("Cannot open media file: {error}"))?;
    let metadata =
        fs::metadata(&path).map_err(|error| format!("Cannot inspect media file: {error}"))?;
    if !metadata.is_file() {
        return Err("Select an audio or video file".into());
    }
    if metadata.len() == 0 {
        return Err("The selected file is empty".into());
    }
    if metadata.len() > MAX_MEDIA_FILE_BYTES {
        return Err("Files larger than 8 GB are not supported".into());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!(
            "Unsupported media format '{}'. Choose WAV, MP3, M4A, AAC, FLAC, OGG, OPUS, WMA, MP4, MOV, M4V, MKV or WebM.",
            extension.to_ascii_uppercase()
        ));
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("media")
        .to_string();
    let is_video = matches!(extension.as_str(), "m4v" | "mkv" | "mov" | "mp4" | "webm");
    Ok((
        path,
        MediaFileInfo {
            name,
            extension,
            size_bytes: metadata.len(),
            is_video,
        },
    ))
}

pub fn split_wav(path: &Path) -> Result<Vec<PathBuf>, String> {
    split_wav_with_limit(path, 16_000 * CHUNK_SECONDS)
}

fn split_wav_with_limit(path: &Path, samples_per_chunk: u64) -> Result<Vec<PathBuf>, String> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|error| format!("Cannot read normalized audio: {error}"))?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != 16_000
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err("Media decoder produced an unsupported audio format".into());
    }
    let total_samples = reader.duration() as u64;
    if total_samples <= samples_per_chunk {
        return Ok(vec![path.to_path_buf()]);
    }

    let mut paths = Vec::new();
    let result = (|| {
        let mut writer: Option<hound::WavWriter<BufWriter<fs::File>>> = None;
        let mut samples_in_chunk = 0_u64;
        for sample in reader.samples::<i16>() {
            if writer.is_none() {
                let chunk_path = crate::error::temp_wav_path();
                writer = Some(
                    hound::WavWriter::create(&chunk_path, spec)
                        .map_err(|error| format!("Cannot create audio chunk: {error}"))?,
                );
                paths.push(chunk_path);
                samples_in_chunk = 0;
            }
            writer
                .as_mut()
                .expect("writer was initialized")
                .write_sample(
                    sample.map_err(|error| format!("Cannot decode normalized audio: {error}"))?,
                )
                .map_err(|error| format!("Cannot write audio chunk: {error}"))?;
            samples_in_chunk += 1;
            if samples_in_chunk >= samples_per_chunk {
                writer
                    .take()
                    .expect("writer was initialized")
                    .finalize()
                    .map_err(|error| format!("Cannot finalize audio chunk: {error}"))?;
            }
        }
        if let Some(writer) = writer {
            writer
                .finalize()
                .map_err(|error| format!("Cannot finalize audio chunk: {error}"))?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        remove_files(&paths);
        return Err(error);
    }
    Ok(paths)
}

pub fn remove_files(paths: &[PathBuf]) {
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

pub fn save_transcript(path: &str, text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("Cannot save an empty transcript".into());
    }
    let path = PathBuf::from(path);
    if path.file_name().is_none() {
        return Err("Choose a destination file".into());
    }
    crate::storage::write_atomic(&path, text.as_bytes())
        .map_err(|error| format!("Cannot save transcript: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_normalized_wav_without_losing_samples() {
        let root = std::env::temp_dir().join(format!(
            "hotyap-media-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&source, spec).unwrap();
        for sample in 0..25 {
            writer.write_sample(sample as i16).unwrap();
        }
        writer.finalize().unwrap();

        let chunks = split_wav_with_limit(&source, 10).unwrap();
        let samples = chunks
            .iter()
            .flat_map(|path| {
                hound::WavReader::open(path)
                    .unwrap()
                    .samples::<i16>()
                    .map(Result::unwrap)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        assert_eq!(chunks.len(), 3);
        assert_eq!(
            samples,
            (0..25).map(|sample| sample as i16).collect::<Vec<_>>()
        );
        remove_files(&chunks);
        fs::remove_dir_all(root).unwrap();
    }
}
