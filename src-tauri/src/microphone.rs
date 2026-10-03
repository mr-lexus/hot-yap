//! Device identities are persisted; an unavailable explicit device never silently
//! changes the recording source. Tests use the same capture path as dictation.
use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
pub struct InputDevice {
    id: String,
    name: String,
    is_default: bool,
}

pub fn resolve_device(selected: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    match selected {
        None => host.default_input_device().ok_or_else(|| "No default microphone is available".into()),
        Some(id) => host.input_devices().map_err(|e| e.to_string())?
            .find(|device| device.id().is_ok_and(|value| value.to_string() == id))
            .ok_or_else(|| "The selected microphone is disconnected. Reconnect it or select System default in Settings.".into()),
    }
}

#[tauri::command]
pub async fn list_microphones() -> Result<Vec<InputDevice>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let host = cpal::default_host();
        let default_id = host.default_input_device().and_then(|d| d.id().ok());
        let devices = host.input_devices().map_err(|e| e.to_string())?;
        Ok(devices
            .filter_map(|d| {
                let id = d.id().ok()?;
                Some(InputDevice {
                    is_default: default_id.as_ref() == Some(&id),
                    id: id.to_string(),
                    name: d.to_string(),
                })
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

struct TestSession {
    id: String,
    started: Instant,
    recorder: crate::audio::Recorder,
}

#[derive(Default)]
pub struct MicrophoneTest(Mutex<Option<TestSession>>);

pub fn stop_all(app: &AppHandle) {
    app.state::<MicrophoneTest>()
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
}

#[derive(Serialize)]
pub struct TestReport {
    active: bool,
    level: f32,
    remaining: u64,
    mic_name: Option<String>,
    error: Option<String>,
}

#[tauri::command]
pub async fn start_microphone_test(
    app: AppHandle,
    id: String,
    device: Option<String>,
) -> Result<(), String> {
    if id.is_empty() || id.len() > 128 {
        return Err("Invalid microphone test session".into());
    }
    let state = app.state::<crate::state::AppState>();
    let inner = state.lock();
    if inner.phase != crate::state::Phase::Idle || inner.closing {
        return Err("Stop dictation before testing the microphone".into());
    }
    drop(inner);
    let tests = app.state::<MicrophoneTest>();
    let mut session = tests.0.lock().unwrap_or_else(|p| p.into_inner());
    session.take();
    let recorder = crate::audio::Recorder::start(device.as_deref())?;
    {
        let inner = state.lock();
        if inner.phase != crate::state::Phase::Idle || inner.closing {
            return Err("The app is no longer idle".into());
        }
    }
    *session = Some(TestSession {
        id: id.clone(),
        started: Instant::now(),
        recorder,
    });
    drop(session);
    // The native deadline also releases the microphone if the UI closes/crashes.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15)).await;
        stop_microphone_test(app, id);
    });
    Ok(())
}

#[tauri::command]
pub fn stop_microphone_test(app: AppHandle, id: String) {
    let tests = app.state::<MicrophoneTest>();
    let mut session = tests.0.lock().unwrap_or_else(|p| p.into_inner());
    if session.as_ref().is_some_and(|s| s.id == id) {
        session.take();
    }
}

#[tauri::command]
pub fn microphone_test_status(app: AppHandle, id: String) -> TestReport {
    let tests = app.state::<MicrophoneTest>();
    let mut session = tests.0.lock().unwrap_or_else(|p| p.into_inner());
    let mut report = TestReport {
        active: false,
        level: 0.0,
        remaining: 0,
        mic_name: None,
        error: None,
    };
    if let Some(test) = session.as_ref().filter(|s| s.id == id) {
        report.error = test.recorder.error();
        report.remaining = 15_u64.saturating_sub(test.started.elapsed().as_secs());
        report.mic_name = test.recorder.mic_name();
        report.active = report.error.is_none() && report.remaining > 0;
        report.level = test.recorder.snapshot().0;
        if !report.active {
            session.take();
        }
    }
    report
}
