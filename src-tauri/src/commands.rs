use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::audio::{write_wav, Recorder};
use crate::error::temp_wav_path;
use crate::providers::{self, ProviderSettings};
use crate::state::{emit_status, AppState, EngineStatus, ModelStatus, Phase};
use crate::worker::{self, request};

fn set(app: &AppHandle, f: impl FnOnce(&mut crate::state::AppStateInner)) {
    let st = app.state::<AppState>();
    let mut inner = st.lock();
    f(&mut inner);
}

#[tauri::command]
pub async fn get_status(app: AppHandle) -> Result<crate::state::StatusReport, String> {
    log::debug!("command: get_status");
    let st = app.state::<AppState>();
    let report = st.lock().report(worker::is_alive(&app));
    Ok(report)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn check_cuda_runtime(app: AppHandle) -> Result<crate::state::CudaRuntimeReport, String> {
    log::debug!("command: check_cuda_runtime");
    if !worker::is_alive(&app) {
        return Ok(app.state::<AppState>().lock().cuda_runtime.clone());
    }

    let models_root = worker::model_dir(&app);
    let response = request(
        &app,
        &*app.state::<Arc<worker::Worker>>(),
        json!({"command": "verify_cuda_runtime", "models_root": models_root}),
        Duration::from_secs(30),
    )
    .await?;

    let gpu_available = response
        .payload
        .get("gpu_available")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let missing: Vec<String> = response
        .payload
        .get("missing")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let runtime_ok = response
        .payload
        .get("runtime_ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(missing.is_empty());

    set(&app, |i| {
        i.cuda_runtime = crate::state::CudaRuntimeReport {
            gpu_available,
            runtime_ok,
            missing,
            progress: None,
            error: None,
        };
    });
    emit_status(&app);
    Ok(app.state::<AppState>().lock().cuda_runtime.clone())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn install_cuda_runtime(app: AppHandle) -> Result<(), String> {
    log::info!("command: install_cuda_runtime");
    {
        let st = app.state::<AppState>();
        let mut inner = st.lock();
        if inner.cuda_runtime.runtime_ok {
            return Ok(());
        }
        if inner.cuda_runtime.progress.is_some() {
            return Err("CUDA runtime download already in progress".into());
        }
        inner.cuda_runtime.progress = Some(0.0);
        inner.cuda_runtime.error = None;
    }
    emit_status(&app);

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let models_root = worker::model_dir(&app2);
        let result = request(
            &app2,
            &*app2.state::<Arc<worker::Worker>>(),
            json!({"command": "download_cuda_runtime", "models_root": models_root}),
            Duration::from_secs(3600),
        )
        .await;
        match result {
            Ok(msg) => {
                let missing: Vec<String> = msg
                    .payload
                    .get("missing")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                log::info!("CUDA runtime download finished; missing: {missing:?}");
                set(&app2, |i| {
                    i.cuda_runtime.progress = None;
                    i.cuda_runtime.missing = missing.clone();
                    i.cuda_runtime.runtime_ok = missing.is_empty();
                    if missing.is_empty() {
                        i.cuda_runtime.error = None;
                    }
                });
            }
            Err(e) => {
                log::error!("CUDA runtime download failed: {e}");
                set(&app2, |i| {
                    i.cuda_runtime.progress = None;
                    i.cuda_runtime.error = Some(e.clone());
                });
            }
        }
        emit_status(&app2);
    });
    Ok(())
}

#[tauri::command]
pub async fn list_models(app: AppHandle) -> Result<Vec<crate::state::ModelInfo>, String> {
    log::debug!("command: list_models");
    let st = app.state::<AppState>();
    let inner = st.lock();
    Ok(inner.models.clone())
}

#[tauri::command]
pub async fn get_provider_settings(app: AppHandle) -> Result<ProviderSettings, String> {
    Ok(app.state::<AppState>().lock().provider_settings.clone())
}

#[tauri::command]
pub async fn save_provider_settings(
    app: AppHandle,
    mut settings: ProviderSettings,
    secrets: HashMap<String, String>,
) -> Result<ProviderSettings, String> {
    if app.state::<AppState>().lock().phase != Phase::Idle {
        return Err("Provider settings can only be changed while the app is idle".into());
    }
    let current = app.state::<AppState>().lock().provider_settings.clone();
    for (id, config) in &mut settings.providers {
        config.api_key_set = current.providers.get(id).map(|saved| saved.api_key_set).unwrap_or(false);
    }
    providers::normalize(&mut settings);
    providers::validate(&settings)?;
    for (provider, secret) in secrets {
        if secret.trim().is_empty() {
            continue;
        }
        providers::store_secret(&provider, &secret)?;
        if let Some(config) = settings.providers.get_mut(&provider) {
            config.api_key_set = true;
        }
    }
    providers::refresh_secret_statuses(&mut settings);
    let (old_device, should_reload) = {
        let st = app.state::<AppState>();
        let inner = st.lock();
        (
            inner.provider_settings.local_device.clone(),
            inner.engine_status == EngineStatus::Ready
                && inner.current_model_id.is_some()
                && inner.provider_settings.stt_provider == "local",
        )
    };
    let path = app.state::<AppState>().lock().provider_settings_path.clone();
    providers::persist_settings(&path, &settings)?;
    set(&app, |inner| {
        inner.provider_settings = settings.clone();
        inner.last_error = None;
    });
    emit_status(&app);

    if should_reload && old_device != settings.local_device {
        if let Some(model_id) = app.state::<AppState>().lock().current_model_id.clone() {
            let app_clone = app.clone();
            let new_dev = settings.local_device.clone();
            tauri::async_runtime::spawn(async move {
                let _ = load_model(app_clone, model_id, Some(new_dev)).await;
            });
        }
    }

    Ok(settings)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_provider_secret(app: AppHandle, provider: String) -> Result<ProviderSettings, String> {
    if app.state::<AppState>().lock().phase != Phase::Idle {
        return Err("Provider credentials can only be changed while the app is idle".into());
    }
    providers::delete_secret(&provider)?;
    let (path, mut settings) = {
        let state = app.state::<AppState>();
        let mut inner = state.lock();
        if let Some(config) = inner.provider_settings.providers.get_mut(&provider) {
            config.api_key_set = false;
        }
        (inner.provider_settings_path.clone(), inner.provider_settings.clone())
    };
    providers::normalize(&mut settings);
    providers::refresh_secret_statuses(&mut settings);
    providers::persist_settings(&path, &settings)?;
    set(&app, |inner| inner.provider_settings = settings.clone());
    emit_status(&app);
    Ok(settings)
}

#[tauri::command]
pub async fn update_model_catalog(app: AppHandle) -> Result<usize, String> {
    log::info!("command: update_model_catalog");
    if !worker::is_alive(&app) {
        return Err("Python engine is not running. Restart the engine first.".into());
    }

    let response = request(
        &app,
        &*app.state::<Arc<worker::Worker>>(),
        json!({
            "command": "discover_models",
            "queries": ["whisper russian", "whisper codeswitch", "faster-whisper"],
            "limit": 24,
        }),
        Duration::from_secs(120),
    )
    .await?;
    let discovered = response
        .payload
        .get("models")
        .cloned()
        .ok_or_else(|| "Model discovery returned no catalog".to_string())?;
    let candidates: Vec<crate::state::ModelInfo> = serde_json::from_value(discovered)
        .map_err(|e| format!("invalid model discovery response: {e}"))?;

    let count = candidates.len();
    let state = app.state::<AppState>();
    let mut inner = state.lock();
    for mut candidate in candidates {
        let model_path = worker::model_dir(&app)
            .join(&candidate.id)
            .join(candidate.ct2_subdir.as_deref().unwrap_or(""));
        candidate.downloaded = model_path.join("model.bin").exists();
        if let Some(existing) = inner.models.iter_mut().find(|model| {
            model.id == candidate.id
                || (model.repo_id == candidate.repo_id && model.ct2_subdir == candidate.ct2_subdir)
        }) {
            candidate.id = existing.id.clone();
            candidate.loaded = existing.loaded;
            candidate.downloaded = existing.downloaded || candidate.downloaded;
            *existing = candidate;
        } else {
            inner.models.push(candidate);
        }
    }
    crate::persist_catalog(&inner.catalog_path, &inner.models)?;
    drop(inner);
    emit_status(&app);
    Ok(count)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn download_model(app: AppHandle, model_id: String) -> Result<(), String> {
    log::info!("command: download_model {}", model_id);

    let (repo_id, ct2_subdir, allow_patterns, revision) = {
        let st = app.state::<AppState>();
        let inner = st.lock();
        let model = inner.models.iter().find(|m| m.id == model_id)
            .ok_or_else(|| format!("Model not found: {}", model_id))?;
        (
            model.repo_id.clone(),
            model.ct2_subdir.clone(),
            model.allow_patterns.clone(),
            model.revision.clone(),
        )
    };

    {
        let st = app.state::<AppState>();
        let mut inner = st.lock();
        if inner.model_status == ModelStatus::Downloading {
            return Err("Download already in progress".into());
        }
        if inner.current_model_id.as_deref() == Some(&model_id) && inner.model_status == ModelStatus::Downloaded {
            return Ok(());
        }
        inner.model_status = ModelStatus::Downloading;
        inner.model_error = None;
        inner.current_model_id = Some(model_id.clone());
        inner.model_progress = Some(0.0);
    }
    emit_status(&app);

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let dir = worker::model_dir(&app2);
        let result = request(
            &app2,
            &*app2.state::<Arc<worker::Worker>>(),
            json!({
                "command": "download_model",
                "model_dir": dir,
                "model_id": model_id,
                "repo_id": repo_id,
                "allow_patterns": allow_patterns,
                "ct2_subdir": ct2_subdir,
                "revision": revision,
            }),
            Duration::from_secs(3600),
        )
        .await;
        match result {
            Ok(_) => {
                log::info!("model download finished: {}", model_id);
                set(&app2, |i| {
                    i.model_status = ModelStatus::Downloaded;
                    i.model_error = None;
                    i.model_progress = None;
                    i.last_error = None;
                    if let Some(m) = i.models.iter_mut().find(|m| m.id == model_id) {
                        m.downloaded = true;
                    }
                });
            }
            Err(e) => {
                log::error!("model download failed: {e}");
                set(&app2, |i| {
                    i.model_status = ModelStatus::Error;
                    i.model_error = Some(e.clone());
                    i.model_progress = None;
                    i.last_error = Some(e);
                });
            }
        }
        emit_status(&app2);
    });
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_model(app: AppHandle, model_id: String) -> Result<(), String> {
    log::info!("command: delete_model {}", model_id);

    {
        let st = app.state::<AppState>();
        let inner = st.lock();
        if !inner.models.iter().any(|model| model.id == model_id) {
            return Err(format!("Model not found: {model_id}"));
        }
        if inner.current_model_id.as_deref() == Some(&model_id)
            && inner.engine_status != EngineStatus::Stopped
        {
            return Err("Cannot delete currently loaded model. Stop the engine first.".into());
        }
    }

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let dir = worker::model_dir(&app2);
        let result = request(
            &app2,
            &*app2.state::<Arc<worker::Worker>>(),
            json!({"command": "delete_model", "model_dir": dir, "model_id": model_id}),
            Duration::from_secs(30),
        )
        .await;
        match result {
            Ok(_) => {
                log::info!("model deleted: {}", model_id);
                set(&app2, |i| {
                    if let Some(m) = i.models.iter_mut().find(|m| m.id == model_id) {
                        m.downloaded = false;
                        m.loaded = false;
                    }
                    if i.current_model_id.as_deref() == Some(&model_id) {
                        i.current_model_id = None;
                        i.model_status = ModelStatus::NotDownloaded;
                        i.engine_status = EngineStatus::Stopped;
                        i.device = None;
                        i.compute_type = None;
                    }
                });
            }
            Err(e) => {
                log::error!("model delete failed: {e}");
                set(&app2, |i| {
                    i.last_error = Some(e);
                });
            }
        }
        emit_status(&app2);
    });
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn load_model(app: AppHandle, model_id: String, device: Option<String>) -> Result<(), String> {
    log::info!("command: load_model {} device={:?}", model_id, device);

    let (ct2_subdir, target_device) = {
        let st = app.state::<AppState>();
        let inner = st.lock();
        let model = inner.models.iter().find(|m| m.id == model_id)
            .ok_or_else(|| format!("Model not found: {}", model_id))?;

        if !model.downloaded {
            return Err("Model is not downloaded yet".into());
        }
        let dev = device.unwrap_or_else(|| inner.provider_settings.local_device.clone());
        (model.ct2_subdir.clone(), dev)
    };

    {
        let st = app.state::<AppState>();
        let mut inner = st.lock();
        if inner.engine_status == EngineStatus::Loading {
            return Err("Model is already loading".into());
        }
        inner.engine_status = EngineStatus::Loading;
        inner.engine_error = None;
        inner.current_model_id = Some(model_id.clone());
    }
    emit_status(&app);

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        // The worker may have exited (crash, previous unload): bring it back
        // before sending the load request, otherwise the request fails with
        // "Python worker is not running".
        if !worker::is_alive(&app2) {
            if let Err(e) = worker::start(&app2).await {
                log::error!("worker restart before load failed: {e}");
                set(&app2, |i| {
                    i.engine_status = EngineStatus::Error;
                    i.engine_error = Some(e.clone());
                    i.last_error = Some(e);
                });
                emit_status(&app2);
                return;
            }
        }
        let dir = worker::model_dir(&app2).join(&model_id);
        let result = request(
            &app2,
            &*app2.state::<Arc<worker::Worker>>(),
            json!({
                "command": "load_model",
                "model_dir": dir,
                "ct2_subdir": ct2_subdir,
                "models_root": worker::model_dir(&app2),
                "device": target_device,
            }),
            Duration::from_secs(900),
        )
        .await;
        match result {
            Ok(msg) => {
                let device = msg
                    .payload
                    .get("device")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string();
                let compute_type = msg
                    .payload
                    .get("compute_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string();
                log::info!("model loaded: device={device} compute_type={compute_type}");
                set(&app2, |i| {
                    i.engine_status = EngineStatus::Ready;
                    i.engine_error = None;
                    i.last_error = None;
                    i.device = Some(device);
                    i.compute_type = Some(compute_type);
                    if let Some(m) = i.models.iter_mut().find(|m| m.id == model_id) {
                        m.loaded = true;
                    }
                });
            }
            Err(e) => {
                log::error!("model load failed: {e}");
                set(&app2, |i| {
                    i.engine_status = EngineStatus::Error;
                    i.engine_error = Some(e.clone());
                    i.last_error = Some(e);
                });
            }
        }
        emit_status(&app2);
    });
    Ok(())
}

#[tauri::command]
pub async fn unload_model(app: AppHandle) -> Result<(), String> {
    log::info!("command: unload_model");

    let current_model = {
        let st = app.state::<AppState>();
        let mut inner = st.lock();
        if inner.engine_status == EngineStatus::Stopped {
            return Ok(());
        }
        inner.engine_status = EngineStatus::Stopped;
        inner.engine_error = None;
        inner.device = None;
        inner.compute_type = None;
        let model_id = inner.current_model_id.take();
        if let Some(id) = &model_id {
            if let Some(m) = inner.models.iter_mut().find(|m| m.id == *id) {
                m.loaded = false;
            }
        }
        model_id
    };

    emit_status(&app);

    // Shut the worker down so the model (and VRAM) is released, wait for the
    // old process to actually exit, then start a fresh worker. Without the
    // restart the engine stays permanently dead: `load_model` cannot talk to
    // a stopped worker and the UI blocks the Load button on worker_alive.
    if current_model.is_some() {
        if let Some(worker_arc) = app.try_state::<Arc<worker::Worker>>() {
            if worker_arc.alive.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = request(
                    &app,
                    &worker_arc,
                    json!({"command": "shutdown"}),
                    Duration::from_secs(3),
                )
                .await;
            }
            // The worker replies shutdown_ack BEFORE the model teardown
            // finishes. Wait for the process to actually die so a subsequent
            // load_model does not hit a half-dead worker whose stdin loop is
            // already gone (which used to hang the load request for minutes).
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            while worker_arc.alive.load(std::sync::atomic::Ordering::SeqCst)
                && tokio::time::Instant::now() < deadline
            {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        let _ = worker::kill(&app).await;
        if let Err(e) = worker::start(&app).await {
            set(&app, |i| {
                i.engine_status = EngineStatus::Error;
                i.engine_error = Some(e.clone());
                i.last_error = Some(e.clone());
            });
            emit_status(&app);
            return Err(format!("Worker failed to restart after model unload: {e}"));
        }
    }

    emit_status(&app);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn set_hotkey(app: AppHandle, shortcut: String) -> Result<(), String> {
    let shortcut = shortcut.trim().to_string();
    if shortcut.is_empty() {
        return Err("Push-to-talk key cannot be empty".into());
    }

    let (old_shortcut, was_registered, phase) = {
        let st = app.state::<AppState>();
        let inner = st.lock();
        (inner.hotkey.clone(), inner.hotkey_registered, inner.phase)
    };
    if phase != Phase::Idle {
        return Err("Change the push-to-talk key when the app is idle".into());
    }
    if shortcut == old_shortcut && was_registered {
        return Ok(());
    }

    let global_shortcut = app.global_shortcut();
    if was_registered {
        global_shortcut
            .unregister(old_shortcut.as_str())
            .map_err(|e| format!("Could not unregister {old_shortcut}: {e}"))?;
    }

    if let Err(e) = global_shortcut.register(shortcut.as_str()) {
        let restore_result = if was_registered {
            global_shortcut.register(old_shortcut.as_str()).err()
        } else {
            None
        };
        let restored = restore_result.is_none();
        let warning = match restore_result {
            Some(restore_error) => format!("Could not register {shortcut}: {e}; restoring {old_shortcut} also failed: {restore_error}"),
            None => format!("Could not register {shortcut}: {e}"),
        };
        set(&app, |i| {
            i.hotkey_warning = Some(warning);
            i.hotkey_registered = was_registered && restored;
        });
        emit_status(&app);
        return Err(format!("Could not register push-to-talk key: {e}"));
    }

    let hotkey_path = app.state::<AppState>().lock().hotkey_path.clone();
    let persistence_error = std::fs::write(&hotkey_path, format!("{shortcut}\n"))
        .err()
        .map(|e| format!("Could not save push-to-talk key: {e}"));
    set(&app, |i| {
        i.hotkey = shortcut.clone();
        i.hotkey_registered = true;
        i.hotkey_warning = persistence_error.clone();
    });
    emit_status(&app);

    persistence_error.map_or(Ok(()), Err)
}

#[tauri::command]
pub async fn start_recording(app: AppHandle) -> Result<(), String> {
    log::info!("command: start_recording");
    {
        let st = app.state::<AppState>();
        let inner = st.lock();
        if inner.phase != Phase::Idle {
            return Err(format!("Cannot start recording while {:?}", inner.phase));
        }
        if !providers::stt_ready(&inner.provider_settings, inner.engine_status == EngineStatus::Ready) {
            return Err("The selected transcription provider is not ready. Open Settings to configure it.".into());
        }
        if providers::provider_needs_key(&inner.provider_settings.stt_provider)
            && !providers::secret_available(&inner.provider_settings.stt_provider)
        {
            return Err("The API key for the selected transcription provider is unavailable".into());
        }
    }

    let recorder = match Recorder::start() {
        Ok(r) => r,
        Err(e) => {
            set(&app, |i| {
                i.last_error = Some(e.clone());
            });
            emit_status(&app);
            return Err(e);
        }
    };

    let mic_name = recorder.mic_name();
    set(&app, |i| {
        i.phase = Phase::Recording;
        i.mic_name = mic_name;
        i.recorder = Some(recorder);
        i.last_error = None;
        i.last_warning = None;
    });

    // Spawn a polling task that emits audio level + spectrum events while recording.
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let (level, spectrum) = {
                let st = app2.state::<AppState>();
                let inner = st.lock();
                if inner.phase != Phase::Recording {
                    break;
                }
                match &inner.recorder {
                    Some(r) => r.snapshot(),
                    None => {
                        set(&app2, |i| {
                            i.audio_level = 0.0;
                            i.audio_spectrum = vec![0.0; 32];
                        });
                        continue;
                    }
                }
            };
            set(&app2, |i| {
                i.audio_level = level;
                i.audio_spectrum = spectrum.clone();
            });
            let _ = app2.emit(
                "vox:audio-meter",
                serde_json::json!({ "level": level, "spectrum": spectrum }),
            );
        }
    });

    emit_status(&app);
    Ok(())
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> Result<String, String> {
    log::info!("command: stop_recording");
    let recorder = {
        let st = app.state::<AppState>();
        let mut inner = st.lock();
        if inner.phase != Phase::Recording {
            return Err("Not recording".into());
        }
        inner.ptt_pressed = false;
        inner.ptt_generation = inner.ptt_generation.wrapping_add(1);
        inner.recorder.take()
    };
    let recorder = match recorder {
        Some(recorder) => recorder,
        None => {
            let error = "Recorder is missing".to_string();
            set(&app, |i| {
                i.phase = Phase::Idle;
                i.last_error = Some(error.clone());
            });
            emit_status(&app);
            return Err(error);
        }
    };

    // Publish the state transition before any potentially blocking stream
    // shutdown or disk work. The UI must never remain in "Recording" here.
    set(&app, |i| i.phase = Phase::Transcribing);
    emit_status(&app);

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let capture = tauri::async_runtime::spawn_blocking(move || recorder.stop()).await;
        let result = match capture {
            Ok(Ok((samples, sample_rate, rec_duration))) => {
                transcribe_recording(app2.clone(), samples, sample_rate, rec_duration).await
            }
            Ok(Err(error)) => {
                set_idle_error(&app2, error.clone());
                Err(error)
            }
            Err(error) => {
                let error = format!("Recording task failed: {error}");
                set_idle_error(&app2, error.clone());
                Err(error)
            }
        };
        if let Err(error) = result {
            log::error!("recording/transcription failed: {error}");
        }
    });

    Ok("Transcription started".to_string())
}

fn set_idle_error(app: &AppHandle, error: String) {
    set(app, |i| {
        i.phase = Phase::Idle;
        i.last_error = Some(error);
        i.last_warning = None;
    });
    emit_status(app);
}

async fn transcribe_recording(
    app: AppHandle,
    samples: Vec<i16>,
    sample_rate: u32,
    rec_duration: f64,
) -> Result<String, String> {
    log::info!("recorded {rec_duration:.1}s of audio");
    if rec_duration < 0.3 || !samples.iter().any(|sample| sample.unsigned_abs() > 64) {
        let error = "Recording was too short or silent".to_string();
        set_idle_error(&app, error.clone());
        return Err(error);
    }

    let wav_path = temp_wav_path();
    if let Err(error) = write_wav(&wav_path, &samples, sample_rate) {
        let _ = std::fs::remove_file(&wav_path);
        set_idle_error(&app, error.clone());
        return Err(error);
    }

    let settings = app.state::<AppState>().lock().provider_settings.clone();
    let local_transcription = settings.stt_provider == "local";
    let result: Result<String, String> = if local_transcription {
        // This box's CPU can run at RTF ~15-20, so a long dictation legitimately
        // takes minutes. Size the timeout from the recorded duration with a wide margin.
        let timeout = Duration::from_secs(
            (rec_duration as u64)
                .saturating_mul(60)
                .saturating_add(300)
                .min(3600),
        );
        request(
            &app,
            &*app.state::<Arc<worker::Worker>>(),
            json!({"command": "transcribe", "audio_path": wav_path}),
            timeout,
        )
        .await
        .map(|msg| {
            let text = msg.payload.get("text").and_then(|value| value.as_str()).unwrap_or("").to_string();
            log::info!(
                "local transcription: audio={:?}s inference={:?}s rtf={:?}",
                msg.payload.get("audio_s").and_then(|value| value.as_f64()),
                msg.payload.get("inference_s").and_then(|value| value.as_f64()),
                msg.payload.get("rtf").and_then(|value| value.as_f64()),
            );
            text
        })
    } else {
        let _ = app.emit("vox:transcribe-progress", json!({ "elapsed": 0, "fraction": 0.12 }));
        providers::transcribe(&settings, &wav_path).await
    };

    let _ = std::fs::remove_file(&wav_path);

    match result {
        Ok(raw_text) => {
            let (text, warning) = if raw_text.trim().is_empty() {
                (raw_text, None)
            } else {
                let _ = app.emit("vox:transcribe-progress", json!({ "elapsed": 0, "fraction": 0.86 }));
                match providers::postprocess(&settings, &raw_text).await {
                    Ok(processed) => (processed, None),
                    Err(error) => {
                        log::warn!("text post-processing failed; using raw transcript: {error}");
                        (raw_text, Some(format!("Text processing failed; the raw transcript was copied: {error}")))
                    }
                }
            };
            if text.trim().is_empty() {
                let e = "No speech detected in the recording".to_string();
                set(&app, |i| {
                    i.phase = Phase::Idle;
                    i.last_error = Some(e.clone());
                });
                emit_status(&app);
                return Err(e);
            }

            // Copy to clipboard (the whole point of the app).
            let copied = match app.clipboard().write_text(&text) {
                Ok(_) => {
                    log::info!("transcription copied to clipboard");
                    true
                }
                Err(e) => {
                    log::error!("clipboard write failed: {e}");
                    false
                }
            };

            set(&app, |i| {
                i.phase = Phase::Idle;
                i.last_text = Some(text.clone());
                i.last_copied = copied;
                i.last_error = None;
                i.last_warning = warning;
            });
            emit_status(&app);
            Ok(text)
        }
        Err(e) => {
            log::error!("transcription failed: {e}");
            // If the worker went unresponsive, kill and respawn it so the app
            // never stays stuck (the old process may still be crunching).
            // Covers both a request timeout ("did not answer") and a broken
            // protocol pipe ("closed the connection"/"not running"), which is
            // what a PyInstaller onefile worker spawned from a GUI parent
            // exhibits after a successful transcription.
            let channel_broken = e.contains("did not answer")
                || e.contains("closed the connection")
                || e.contains("is not running")
                || e.contains("Failed to write to worker");
            if local_transcription && channel_broken {
                log::warn!("worker channel broken; restarting the worker");
                let _ = worker::kill(&app).await;
                if worker::start(&app).await.is_ok() {
                    // The fresh worker has no model loaded; reload the
                    // previously loaded one so the next dictation just works.
                    let model_id = app
                        .state::<AppState>()
                        .lock()
                        .current_model_id
                        .clone();
                    if let Some(model_id) = model_id {
                        let _ = load_model(app.clone(), model_id, None).await;
                    }
                }
            }
            set(&app, |i| {
                i.phase = Phase::Idle;
                i.last_error = Some(e.clone());
            });
            emit_status(&app);
            Err(e)
        }
    }
}

pub fn mark_ptt_pressed(app: &AppHandle) -> Result<Option<u64>, String> {
    let st = app.state::<AppState>();
    let mut inner = st.lock();
    if inner.phase == Phase::Transcribing {
        return Err("Still transcribing the previous recording".into());
    }
    if inner.ptt_pressed {
        return Ok(None);
    }
    inner.ptt_pressed = true;
    inner.ptt_generation = inner.ptt_generation.wrapping_add(1);
    Ok(Some(inner.ptt_generation))
}

pub fn mark_ptt_released(app: &AppHandle) -> Option<u64> {
    let st = app.state::<AppState>();
    let mut inner = st.lock();
    if !inner.ptt_pressed {
        return None;
    }
    inner.ptt_pressed = false;
    inner.ptt_generation = inner.ptt_generation.wrapping_add(1);
    Some(inner.ptt_generation)
}

fn should_finish_released_recording(app: &AppHandle, generation: u64) -> bool {
    let st = app.state::<AppState>();
    let inner = st.lock();
    inner.ptt_generation != generation
        && !inner.ptt_pressed
        && inner.phase == Phase::Recording
}

pub async fn start_recording_after_ptt(
    app: AppHandle,
    generation: u64,
) -> Result<(), String> {
    let result = start_recording(app.clone()).await;

    if let Err(error) = result {
        set(&app, |i| {
            if i.ptt_generation == generation {
                i.ptt_pressed = false;
            }
        });
        return Err(error);
    }

    // A very quick press can release before Recorder::start() finishes.
    // Do not leave that short recording running in the background.
    if should_finish_released_recording(&app, generation) {
        stop_recording(app).await.map(|_| ())
    } else {
        Ok(())
    }
}

pub async fn stop_recording_after_ptt(app: AppHandle) -> Result<(), String> {
    let recording = {
        let st = app.state::<AppState>();
        let phase = st.lock().phase;
        phase == Phase::Recording
    };
    if recording {
        stop_recording(app).await.map(|_| ())
    } else {
        Ok(())
    }
}

#[tauri::command]
pub async fn press_to_talk(app: AppHandle) -> Result<(), String> {
    let generation = match mark_ptt_pressed(&app)? {
        Some(generation) => generation,
        None => return Ok(()),
    };
    start_recording_after_ptt(app, generation).await
}

#[tauri::command]
pub async fn release_to_talk(app: AppHandle) -> Result<(), String> {
    if mark_ptt_released(&app).is_none() {
        return Ok(());
    }
    stop_recording_after_ptt(app).await
}

#[tauri::command]
pub async fn restart_worker(app: AppHandle) -> Result<(), String> {
    log::info!("command: restart_worker");
    let _ = worker::kill(&app).await;
    match worker::start(&app).await {
        Ok(()) => {
            set(&app, |i| {
                i.engine_status = EngineStatus::Stopped;
                i.engine_error = None;
                i.device = None;
                i.compute_type = None;
                i.phase = Phase::Idle;
                i.ptt_pressed = false;
                i.ptt_generation = i.ptt_generation.wrapping_add(1);
                i.last_error = None;
            });
            emit_status(&app);
            // Re-check whether the CUDA runtime became usable (e.g. after a
            // runtime download finished); harmless when nothing changed.
            let _ = check_cuda_runtime(app.clone()).await;
            Ok(())
        }
        Err(e) => {
            set(&app, |i| {
                i.engine_status = EngineStatus::Error;
                i.engine_error = Some(e.clone());
                i.ptt_pressed = false;
                i.ptt_generation = i.ptt_generation.wrapping_add(1);
                i.last_error = Some(e.clone());
            });
            emit_status(&app);
            Err(e)
        }
    }
}
