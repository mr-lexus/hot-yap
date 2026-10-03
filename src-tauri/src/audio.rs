use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, StreamTrait};

const MAX_RECORDING_SECONDS: usize = 30 * 60;
const MAX_RECORDING_BUFFER_BYTES: usize = 256 * 1024 * 1024;

static INPUT_IN_USE: AtomicBool = AtomicBool::new(false);
struct InputLease;
impl Drop for InputLease {
    fn drop(&mut self) {
        INPUT_IN_USE.store(false, Ordering::SeqCst);
    }
}

/// Microphone recorder built on cpal.
///
/// Samples are stored interleaved as i16 in memory while recording.
/// After `stop()` the buffer is downmixed to mono and written to a WAV
/// file with the device's actual sample rate.
pub struct Recorder {
    data: Arc<Mutex<Vec<i16>>>,
    channels: u16,
    sample_rate: u32,
    stream: cpal::Stream,
    started: Instant,
    error: Arc<Mutex<Option<String>>>,
    mic_name: String,
    level: Arc<AtomicUsize>,
    output_guard: Option<crate::system_audio::OutputGuard>,
    _lease: InputLease,
}

impl Recorder {
    pub fn capture(&self) -> AudioCapture {
        AudioCapture {
            data: self.data.clone(),
            channels: self.channels as usize,
            sample_rate: self.sample_rate,
        }
    }
    pub fn mic_name(&self) -> Option<String> {
        if self.mic_name.is_empty() {
            None
        } else {
            Some(self.mic_name.clone())
        }
    }

    pub fn start(selected: Option<&str>) -> Result<Recorder, String> {
        INPUT_IN_USE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "Microphone is already in use by HotYap".to_string())?;
        let lease = InputLease;
        let device = crate::microphone::resolve_device(selected)?;
        let config = device
            .default_input_config()
            .map_err(|e| format!("Cannot read microphone input config: {e}"))?;

        let channels = config.channels();
        let sample_rate = config.sample_rate();
        let mic_name = device.to_string();
        log::info!(
            "microphone '{mic_name}' selected: {sample_rate} Hz, {channels} channel(s), {:?}",
            config.sample_format()
        );

        let data = Arc::new(Mutex::new(Vec::<i16>::new()));
        let error = Arc::new(Mutex::new(None::<String>));
        let level = Arc::new(AtomicUsize::new(0));
        let duration_samples = (sample_rate as usize)
            .saturating_mul(channels as usize)
            .saturating_mul(MAX_RECORDING_SECONDS);
        let memory_samples = MAX_RECORDING_BUFFER_BYTES / std::mem::size_of::<i16>();
        let max_samples = duration_samples.min(memory_samples);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                build_stream::<f32>(&device, config.config(), &data, &error, &level, max_samples)?
            }
            cpal::SampleFormat::I16 => {
                build_stream::<i16>(&device, config.config(), &data, &error, &level, max_samples)?
            }
            cpal::SampleFormat::U16 => {
                build_stream::<u16>(&device, config.config(), &data, &error, &level, max_samples)?
            }
            other => {
                return Err(format!(
                    "Unsupported microphone sample format: {other:?} \
                     (supported: f32, i16, u16)"
                ))
            }
        };

        stream
            .play()
            .map_err(|e| format!("Failed to start microphone stream: {e}"))?;

        Ok(Recorder {
            data,
            channels,
            sample_rate,
            stream,
            started: Instant::now(),
            error,
            mic_name,
            level,
            output_guard: None,
            _lease: lease,
        })
    }

    /// Stop the stream and return (interleaved mono-mix i16 samples, sample rate, duration s).
    pub fn stop(self) -> Result<(Vec<i16>, u32, f64), String> {
        // Stop callbacks before reading their error and sample buffers so a
        // final callback cannot race with the snapshot below.
        drop(self.stream);
        drop(self.output_guard);
        let mic_error = self
            .error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        let mut buf = self
            .data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let samples = std::mem::take(&mut *buf);
        let duration = self.started.elapsed().as_secs_f64();
        if let Some(e) = mic_error {
            return Err(format!("Microphone error during recording: {e}"));
        }
        if samples.is_empty() {
            return Err("No audio captured (microphone silent or closed)".to_string());
        }
        let mono: Vec<i16> = if self.channels == 1 {
            samples
        } else {
            samples
                .chunks(self.channels as usize)
                .map(|frame| {
                    let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                    (sum / self.channels as i32) as i16
                })
                .collect()
        };
        Ok((mono, self.sample_rate, duration))
    }

    pub fn control_output(&mut self, mode: crate::providers::SystemAudio) -> Result<(), String> {
        self.output_guard = crate::system_audio::OutputGuard::start(mode)?;
        Ok(())
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Snapshot the current audio level and a cheap activity meter.
    pub fn snapshot(&self) -> (f32, Vec<f32>) {
        let lv = self.level.load(Ordering::Relaxed) as f32 / 32768.0;
        let sp = (0..32)
            .map(|i| {
                let shape = 0.55 + ((i * 17 % 11) as f32 / 20.0);
                (lv * shape * 4.0).clamp(0.0, 1.0)
            })
            .collect();
        (lv, sp)
    }
}

#[derive(Clone)]
pub struct AudioCapture {
    data: Arc<Mutex<Vec<i16>>>,
    channels: usize,
    pub sample_rate: u32,
}
impl AudioCapture {
    pub fn tail(&self, seconds: usize) -> (Vec<i16>, usize) {
        let data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        let total = data.len() / self.channels;
        let start = total.saturating_sub(self.sample_rate as usize * seconds);
        let samples = data[start * self.channels..]
            .chunks_exact(self.channels)
            .map(|frame| {
                (frame.iter().map(|&s| s as i64).sum::<i64>() / self.channels as i64) as i16
            })
            .collect();
        (samples, total)
    }
    pub fn from(&self, start: usize) -> Vec<i16> {
        let data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        data.get(start.saturating_mul(self.channels)..)
            .unwrap_or_default()
            .chunks_exact(self.channels)
            .take(self.sample_rate as usize * 60)
            .map(|frame| {
                (frame.iter().map(|&s| s as i64).sum::<i64>() / self.channels as i64) as i16
            })
            .collect()
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    data: &Arc<Mutex<Vec<i16>>>,
    error: &Arc<Mutex<Option<String>>>,
    level: &Arc<AtomicUsize>,
    max_samples: usize,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    i16: cpal::FromSample<T>,
{
    let data = data.clone();
    let error = error.clone();
    let callback_error = error.clone();
    let level = level.clone();

    device
        .build_input_stream(
            config,
            move |buf: &[T], _| {
                let mut out = data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if out.len().saturating_add(buf.len()) > max_samples {
                    if let Ok(mut error) = callback_error.lock() {
                        error.get_or_insert_with(|| {
                            format!(
                                "Recording exceeded the {} minute or {} MB safety limit",
                                MAX_RECORDING_SECONDS / 60,
                                MAX_RECORDING_BUFFER_BYTES / (1024 * 1024)
                            )
                        });
                    }
                    return;
                }
                let mut sum_sq: i64 = 0;
                let mut cnt: i64 = 0;
                for s in buf {
                    let sample = s.to_sample::<i16>();
                    out.push(sample);
                    sum_sq += (sample as i64) * (sample as i64);
                    cnt += 1;
                }
                if cnt > 0 {
                    let rms = (sum_sq as f64 / cnt as f64).sqrt() as f32;
                    level.store(rms as usize, Ordering::Relaxed);
                }
            },
            move |e| {
                log::error!("microphone stream error: {e}");
                *error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(e.to_string());
            },
            None,
        )
        .map_err(|e| format!("Failed to open microphone stream: {e}"))
}

/// Write mono i16 samples as a 16-bit PCM WAV with the given sample rate.
pub fn write_wav(path: &Path, samples: &[i16], sample_rate: u32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| format!("Failed to create WAV file {}: {e}", path.display()))?;
    for &s in samples {
        writer
            .write_sample(s)
            .map_err(|e| format!("Failed to write WAV sample: {e}"))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("Failed to finalize WAV file: {e}"))?;
    log::info!(
        "wrote WAV {} ({} samples, {} Hz, {:.1}s)",
        path.display(),
        samples.len(),
        sample_rate,
        samples.len() as f64 / sample_rate as f64
    );
    Ok(())
}

/// Resample mono i16 samples to 16 kHz (Whisper's expected sample rate).
/// Uses a windowed-sinc resampler (rubato) with a linear-interpolation
/// fallback for buffers too short for the sinc filter.
pub fn resample_to_16k(samples: &[i16], from_hz: u32) -> Vec<i16> {
    if from_hz == 16_000 || samples.len() < 2 {
        return samples.to_vec();
    }
    let input: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    let output = sinc_resample(&input, from_hz, 16_000)
        .unwrap_or_else(|| linear_resample(&input, from_hz, 16_000));
    output
        .into_iter()
        .map(|v| (v * 32768.0).round().clamp(-32768.0, 32767.0) as i16)
        .collect()
}

fn sinc_resample(input: &[f32], from_hz: u32, to_hz: u32) -> Option<Vec<f32>> {
    use rubato::{
        Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
    };
    if input.len() < 256 {
        return None;
    }
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    let mut resampler =
        SincFixedIn::<f32>::new(to_hz as f64 / from_hz as f64, 2.0, params, 1024, 1).ok()?;
    let expected_len = (input.len() as f64 * to_hz as f64 / from_hz as f64).round() as usize;
    let mut output = Vec::with_capacity(expected_len + 1024);
    for chunk in input.chunks(1024) {
        let processed = resampler.process_partial(Some(&[chunk]), None).ok()?;
        output.extend_from_slice(&processed[0]);
    }
    // A sinc filter holds the end of the recording in its internal buffer.
    // Rubato 0.15's SincFixedIn starts at a negative filter index, so its
    // lookahead delays output availability without inserting leading silence.
    while output.len() < expected_len {
        let processed = resampler.process_partial::<&[f32]>(None, None).ok()?;
        if processed[0].is_empty() {
            return None;
        }
        output.extend_from_slice(&processed[0]);
    }
    output.truncate(expected_len);
    Some(output)
}

fn linear_resample(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    let ratio = to_hz as f64 / from_hz as f64;
    let out_len = ((input.len() as f64) * ratio).round().max(1.0) as usize;
    let mut output = Vec::with_capacity(out_len);
    let max_index = input.len().saturating_sub(1);
    for i in 0..out_len {
        let position = i as f64 / ratio;
        let lo = (position.floor() as usize).min(max_index);
        let hi = (lo + 1).min(max_index);
        let frac = (position - lo as f64) as f32;
        output.push(input[lo] * (1.0 - frac) + input[hi] * frac);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_is_identity_at_16k() {
        let samples: Vec<i16> = (0..16_000)
            .map(|i| ((i as f32 * 0.05).sin() * 12_000.0) as i16)
            .collect();
        let out = resample_to_16k(&samples, 16_000);
        assert_eq!(out, samples);
    }

    #[test]
    fn resample_48k_to_16k_preserves_frequency() {
        let rate = 48_000u32;
        let freq = 1_000.0f32;
        let samples: Vec<i16> = (0..rate)
            .map(|i| {
                ((2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 20_000.0)
                    as i16
            })
            .collect();
        let out = resample_to_16k(&samples, rate);
        assert!(
            (out.len() as i64 - 16_000).abs() < 64,
            "unexpected length {}",
            out.len()
        );
        let crossings = out.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
        let estimated_hz = crossings as f32 / 2.0;
        assert!(
            (estimated_hz - freq).abs() < 80.0,
            "estimated frequency {estimated_hz} Hz, expected {freq} Hz"
        );
    }

    #[test]
    fn resample_preserves_duration_and_tail() {
        for rate in [8_000, 44_100, 48_000, 96_000] {
            let mut samples = vec![0; rate as usize];
            let tail_start = samples.len() - (rate / 1000) as usize;
            samples[tail_start..].fill(20_000);
            let out = resample_to_16k(&samples, rate);
            assert_eq!(out.len(), 16_000, "sample rate {rate}");
            assert!(out[15_990] > 15_000, "recording tail lost at {rate} Hz");
        }
    }

    #[test]
    fn resample_handles_short_and_empty_recordings() {
        assert!(resample_to_16k(&[], 48_000).is_empty());
        let out = resample_to_16k(&[12_000; 48], 48_000);
        assert_eq!(out, vec![12_000; 16]);
    }
}
