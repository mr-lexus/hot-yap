//! Incremental local dictation. Only chunks ending in a real pause are committed.
//! A speculative preview is never appended to the final transcript. The original
//! audio remains available until finalization, so a failed chunk cannot lose speech.
use crate::{
    audio::{resample_to_16k, write_wav, AudioCapture},
    dictionary::Dictionary,
    state::{emit_status, AppState, Phase},
    worker::{self, Worker},
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

#[derive(Default)]
pub struct LiveResult {
    pub offset: usize,
    pub text: String,
    pub failure: Option<String>,
}

// 800 ms of quiet with 100 ms lookahead. Keep half the quiet on each side.
// No forced cuts in continuous speech, no punctuation-based string stitching.
pub fn pause_boundary(samples: &[i16], rate: usize) -> Option<usize> {
    if rate == 0 {
        return None;
    }
    let frame = (rate / 50).max(1);
    let mut quiet = 0;
    let mut speech = false;
    for (i, block) in samples.chunks_exact(frame).enumerate() {
        let rms =
            (block.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / block.len() as f64).sqrt();
        if rms < 100.0 {
            quiet += 1;
        } else {
            quiet = 0;
            speech = true;
        }
        let end = (i + 1) * frame;
        if speech && quiet >= 40 && end >= 4 * rate && end + rate / 10 <= samples.len() {
            return Some(end - rate * 2 / 5);
        }
    }
    None
}

pub async fn transcribe_chunk(
    app: &AppHandle,
    samples: Vec<i16>,
    rate: u32,
    dict: &Dictionary,
    context: &str,
) -> Result<String, String> {
    let path = crate::error::temp_wav_path();
    let duration = samples.len() as u64 / rate.max(1) as u64;
    let wav = path.clone();
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        let audio = if rate == 16000 {
            samples
        } else {
            resample_to_16k(&samples, rate)
        };
        write_wav(&wav, &audio, 16000)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    if let Err(e) = prepared {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    let rid = worker::next_request_id(&app.state::<Arc<Worker>>());
    let cancelled = {
        let state = app.state::<AppState>();
        let mut inner = state.lock();
        inner.transcribe_request_id = Some(rid);
        inner.transcribe_cancel.load(Ordering::SeqCst)
    };
    let result = if cancelled {
        Err("Transcription cancelled".into())
    } else {
        worker::request_with_id(app, &app.state::<Arc<Worker>>(), serde_json::json!({
            "command": "transcribe", "audio_path": path, "vocabulary": dict.hints(), "context": context, "term_fixes": false,
        }), Duration::from_secs((duration * 60 + 300).min(3600)), Some(rid)).await.map(|m| m.payload["text"].as_str().unwrap_or_default().to_string())
    };
    let _ = std::fs::remove_file(&path);
    {
        let state = app.state::<AppState>();
        let mut inner = state.lock();
        if inner.transcribe_request_id == Some(rid) {
            inner.transcribe_request_id = None;
        }
    }
    result
}

pub fn start(
    app: AppHandle,
    audio: AudioCapture,
    dict: Dictionary,
    stop: Arc<AtomicBool>,
) -> tokio::sync::oneshot::Receiver<LiveResult> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    tauri::async_runtime::spawn(async move {
        let mut result = LiveResult::default();
        let mut last_preview = 0;
        let mut preview_allowed = true;
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let valid = {
                let state = app.state::<AppState>();
                let inner = state.lock();
                inner.phase == Phase::Recording && !inner.transcribe_cancel.load(Ordering::SeqCst)
            };
            if !valid {
                break;
            }
            let capture = audio.clone();
            let offset = result.offset;
            let mut samples =
                match tauri::async_runtime::spawn_blocking(move || capture.from(offset)).await {
                    Ok(v) => v,
                    Err(_) => break,
                };
            if !samples.iter().any(|s| s.unsigned_abs() > 64) {
                continue;
            }
            let boundary = pause_boundary(&samples, audio.sample_rate as usize);
            let mut available = samples.len();
            let mut tail_preview = false;
            if boundary.is_none()
                && preview_allowed
                && samples.len() > audio.sample_rate as usize * 24
            {
                let capture = audio.clone();
                if let Ok((tail, total)) =
                    tauri::async_runtime::spawn_blocking(move || capture.tail(24)).await
                {
                    samples = tail;
                    available = total.saturating_sub(result.offset);
                    tail_preview = true;
                }
            }
            let preview = boundary.is_none()
                && preview_allowed
                && available >= last_preview + audio.sample_rate as usize * 6;
            if boundary.is_none() && !preview {
                continue;
            }
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let end = boundary.unwrap_or(samples.len());
            let start = Instant::now();
            let context: String = result
                .text
                .chars()
                .rev()
                .take(220)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            match transcribe_chunk(
                &app,
                samples[..end].to_vec(),
                audio.sample_rate,
                &dict,
                &context,
            )
            .await
            {
                Ok(text) => {
                    if boundary.is_some() {
                        // An empty decode is not proof that the captured speech was empty.
                        // Leave this audio uncommitted for the final pass to retry.
                        if text.trim().is_empty() {
                            break;
                        }
                        result.text = join(&result.text, &text);
                        result.offset += end;
                        last_preview = 0;
                    } else {
                        last_preview = available;
                    }
                    // Slow hardware gets useful committed chunks without repeated speculative work.
                    if start.elapsed().as_secs_f64() > end as f64 / audio.sample_rate as f64 {
                        preview_allowed = false;
                    }
                    let shown = if boundary.is_some() {
                        result.text.clone()
                    } else if tail_preview {
                        join(&result.text, &format!("… {text}"))
                    } else {
                        join(&result.text, &text)
                    };
                    {
                        let state = app.state::<AppState>();
                        let mut inner = state.lock();
                        if inner
                            .live_stop
                            .as_ref()
                            .is_some_and(|s| Arc::ptr_eq(s, &stop))
                            && !inner.transcribe_cancel.load(Ordering::SeqCst)
                        {
                            inner.live_text = dict.apply(&shown);
                        }
                    }
                    emit_status(&app);
                }
                Err(error) => {
                    log::warn!(
                        "Live preview stopped: {error}; remaining audio will be finalized normally"
                    );
                    if [
                        "did not answer",
                        "closed the connection",
                        "is not running",
                        "Failed to write to worker",
                        "transcription timed out",
                    ]
                    .iter()
                    .any(|s| error.contains(s))
                    {
                        result.failure = Some(error);
                    }
                    break;
                }
            }
        }
        let _ = sender.send(result);
    });
    receiver
}
pub fn join(a: &str, b: &str) -> String {
    [a.trim(), b.trim()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_cut_inside_speech_or_short_pause() {
        assert_eq!(pause_boundary(&vec![1000; 8000], 1000), None);
        let mut audio = vec![1000; 4000];
        audio.extend(vec![0; 500]);
        audio.extend(vec![1000; 2000]);
        assert_eq!(pause_boundary(&audio, 1000), None);
    }
    #[test]
    fn pause_cuts_cover_audio_exactly_once() {
        let mut audio = vec![1000; 4000];
        audio.extend(vec![0; 1000]);
        audio.extend(vec![1000; 1000]);
        let boundary = pause_boundary(&audio, 1000).unwrap();
        assert!(boundary >= 4000 && boundary <= 5000);
        assert_eq!(
            [audio[..boundary].to_vec(), audio[boundary..].to_vec()].concat(),
            audio
        );
        assert_eq!(pause_boundary(&vec![0; 6000], 1000), None);
    }
    #[test]
    fn join_preserves_punctuation_and_repetitions() {
        assert_eq!(join("Да, да,", "это так."), "Да, да, это так.");
    }
}
