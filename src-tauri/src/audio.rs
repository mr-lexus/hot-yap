use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

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
    level_stop: Arc<AtomicUsize>,
}

impl Recorder {
    pub fn mic_name(&self) -> Option<String> {
        if self.mic_name.is_empty() {
            None
        } else {
            Some(self.mic_name.clone())
        }
    }

    pub fn start() -> Result<Recorder, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "No default input device found (microphone unavailable)".to_string())?;
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
        let level_stop = Arc::new(AtomicUsize::new(0));

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                build_stream::<f32>(&device, config.config(), &data, &error, &level)?
            }
            cpal::SampleFormat::I16 => {
                build_stream::<i16>(&device, config.config(), &data, &error, &level)?
            }
            cpal::SampleFormat::U16 => {
                build_stream::<u16>(&device, config.config(), &data, &error, &level)?
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
            level_stop,
        })
    }

    /// Stop the stream and return (interleaved mono-mix i16 samples, sample rate, duration s).
    pub fn stop(self) -> Result<(Vec<i16>, u32, f64), String> {
        self.level_stop.store(1, Ordering::SeqCst);
        let mic_error = self.error.lock().unwrap().take();
        drop(self.stream);
        let mut buf = self.data.lock().unwrap();
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

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    data: &Arc<Mutex<Vec<i16>>>,
    error: &Arc<Mutex<Option<String>>>,
    level: &Arc<AtomicUsize>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    i16: cpal::FromSample<T>,
{
    let data = data.clone();
    let error = error.clone();
    let level = level.clone();

    device
        .build_input_stream(
            config,
            move |buf: &[T], _| {
                let mut out = match data.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
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
                *error.lock().unwrap() = Some(e.to_string());
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
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    let mut resampler = SincFixedIn::<f32>::new(
        to_hz as f64 / from_hz as f64,
        2.0,
        params,
        input.len(),
        1,
    )
    .ok()?;
    let output = resampler.process(&[input.to_vec()], None).ok()?;
    output.into_iter().next()
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
        let crossings = out
            .windows(2)
            .filter(|w| (w[0] < 0) != (w[1] < 0))
            .count();
        let estimated_hz = crossings as f32 / 2.0;
        assert!(
            (estimated_hz - freq).abs() < 80.0,
            "estimated frequency {estimated_hz} Hz, expected {freq} Hz"
        );
    }
}
