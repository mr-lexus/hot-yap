use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::audio::{write_wav, Recorder};
use crate::error::temp_wav_path;
use crate::history::{HistoryEntry, HistoryStore};
use crate::media::{self, MediaFileInfo};
use crate::providers::{self, ProviderSettings};
use crate::state::{emit_status, AppState, EngineStatus, ModelStatus, Phase};
use crate::worker::{self, request, request_with_id};

fn set(app: &AppHandle, f: impl FnOnce(&mut crate::state::AppStateInner)) {
    let st = app.state::<AppState>();
    let mut inner = st.lock();
    f(&mut inner);
}

fn device_matches_backend(backend: &str, device: &str) -> bool {
    match backend {
        "mlx" => matches!(device, "auto" | "metal"),
        "ctranslate2" => matches!(device, "auto" | "cpu" | "cuda"),
        _ => false,
    }
}

#[cfg(test)]
mod device_tests {
    use super::device_matches_backend;

    #[test]
    fn compute_devices_match_model_backends() {
        assert!(device_matches_backend("mlx", "auto"));
        assert!(device_matches_backend("mlx", "metal"));
        assert!(!device_matches_backend("mlx", "cpu"));
        assert!(device_matches_backend("ctranslate2", "cpu"));
        assert!(device_matches_backend("ctranslate2", "cuda"));
        assert!(!device_matches_backend("ctranslate2", "metal"));
        assert!(!device_matches_backend("unknown", "auto"));
    }
}

#[tauri::command]
pub async fn get_status(app: AppHandle) -> Result<crate::state::StatusReport, String> {
    log::debug!("command: get_status");
    let st = app.state::<AppState>();
    let report = st.lock().report(worker::is_alive(&app));
    Ok(report)
}

#[tauri::command]
pub async fn get_history(app: AppHandle) -> Result<Vec<HistoryEntry>, String> {
    Ok(app.state::<HistoryStore>().list())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn set_history_favorite(
    app: AppHandle,
    id: String,
    favorite: bool,
) -> Result<HistoryEntry, String> {
    app.state::<HistoryStore>().set_favorite(&id, favorite)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_history_entry(app: AppHandle, id: String) -> Result<(), String> {
    app.state::<HistoryStore>().delete(&id)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn clear_history(app: AppHandle, keep_favorites: bool) -> Result<usize, String> {
    app.state::<HistoryStore>().clear(keep_favorites)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn copy_history_entry(app: AppHandle, id: String) -> Result<(), String> {
    let entry = app
        .state::<HistoryStore>()
        .get(&id)
        .ok_or_else(|| "History entry was not found".to_string())?;
    app.clipboard()
        .write_text(entry.text)
        .map_err(|error| format!("Cannot copy history entry: {error}"))
}

#[derive(Debug, Serialize)]
pub struct MediaTranscriptionResult {
    pub text: String,
    pub file_name: String,
    pub duration: f64,
    pub has_video: bool,
    pub warning: Option<String>,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn inspect_media_file(path: String) -> Result<MediaFileInfo, String> {
    media::inspect(&path).map(|(_, info)| info)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn copy_transcript_text(app: AppHandle, text: String) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("Cannot copy an empty transcript".into());
    }
    app.clipboard()
        .write_text(text)
        .map_err(|error| format!("Cannot copy transcript: {error}"))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_transcript_file(path: String, text: String) -> Result<(), String> {
    media::save_transcript(&path, &text)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn transcribe_media_file(
    app: AppHandle,
    path: String,
) -> Result<MediaTranscriptionResult, String> {
    let (source_path, file_info) = media::inspect(&path)?;
    if !worker::is_alive(&app) {
        return Err(
            "The media decoder is unavailable. Restart or install the engine first.".into(),
        );
    }
    if worker::is_busy(&app) {
        return Err("The engine is busy with another operation".into());
    }

    let (settings, history_model, cancelled) = {
        let state = app.state::<AppState>();
        let mut inner = state.lock();
        if inner.phase != Phase::Idle {
            return Err("Another recording or transcription is already in progress".into());
        }
        if !providers::stt_ready(
            &inner.provider_settings,
            inner.engine_status == EngineStatus::Ready,
        ) {
            return Err(
                "The selected transcription provider is not ready. Configure it first.".into(),
            );
        }
        let settings = inner.provider_settings.clone();
        let history_model = selected_transcription_model(&inner, &settings);
        inner.phase = Phase::Transcribing;
        inner.transcribe_cancel.store(false, Ordering::SeqCst);
        inner.transcribe_progress = Some(0.0);
        inner.transcribe_elapsed = 0.0;
        inner.media_progress_range = None;
        inner.last_error = None;
        inner.last_warning = None;
        (settings, history_model, inner.transcribe_cancel.clone())
    };
    if providers::provider_needs_key(&settings.stt_provider)
        && !providers::secret_available(&settings.stt_provider)
    {
        set(&app, |inner| {
            inner.phase = Phase::Idle;
            inner.transcribe_request_id = None;
            inner.media_progress_range = None;
            inner.transcribe_progress = None;
            inner.transcribe_elapsed = 0.0;
        });
        emit_status(&app);
        return Err("The API key for the selected transcription provider is unavailable".into());
    }

    emit_status(&app);
    let _ = app.emit(
        "vox:file-progress",
        json!({ "stage": "decoding", "fraction": 0.0 }),
    );

    let normalized_path = temp_wav_path();
    let result = transcribe_prepared_media(
        &app,
        &source_path,
        &normalized_path,
        &file_info,
        &settings,
        &history_model,
        &cancelled,
    )
    .await;

    set(&app, |inner| {
        inner.phase = Phase::Idle;
        inner.transcribe_request_id = None;
        inner.media_progress_range = None;
        inner.transcribe_progress = None;
        inner.transcribe_elapsed = 0.0;
        match &result {
            Ok(value) => {
                inner.last_error = None;
                inner.last_warning = value.warning.clone();
            }
            Err(error) if error == crate::cancellation::CANCELLED => {
                inner.last_error = None;
            }
            Err(error) => {
                inner.last_error = Some(error.clone());
            }
        }
    });
    emit_status(&app);
    result
}

async fn transcribe_prepared_media(
    app: &AppHandle,
    source_path: &Path,
    normalized_path: &Path,
    file_info: &MediaFileInfo,
    settings: &ProviderSettings,
    history_model: &str,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<MediaTranscriptionResult, String> {
    let mut temporary_files = vec![normalized_path.to_path_buf()];
    let result = async {
        let request_id = worker::next_request_id(&app.state::<Arc<worker::Worker>>());
        set(app, |inner| inner.transcribe_request_id = Some(request_id));
        let prepared = request_with_id(
            app,
            &app.state::<Arc<worker::Worker>>(),
            json!({
                "command": "prepare_media",
                "input_path": source_path,
                "output_path": normalized_path,
            }),
            Duration::from_secs(60 * 60),
            Some(request_id),
        )
        .await;
        set(app, |inner| inner.transcribe_request_id = None);
        let prepared = prepared?;
        check_cancelled(cancelled)?;

        let duration = prepared
            .payload
            .get("duration")
            .and_then(|value| value.as_f64())
            .unwrap_or_default();
        let has_video = prepared
            .payload
            .get("has_video")
            .and_then(|value| value.as_bool())
            .unwrap_or(file_info.is_video);

        let normalized_for_split = normalized_path.to_path_buf();
        let chunks =
            tauri::async_runtime::spawn_blocking(move || media::split_wav(&normalized_for_split))
                .await
                .map_err(|error| format!("Audio preparation task failed: {error}"))??;
        for chunk in &chunks {
            if chunk != normalized_path {
                temporary_files.push(chunk.clone());
            }
        }
        if chunks.is_empty() {
            return Err("The selected file contains no decodable audio".into());
        }

        let mut text_parts = Vec::new();
        let mut warnings = Vec::new();
        let chunk_span = 0.74_f32 / chunks.len() as f32;
        for (index, chunk) in chunks.iter().enumerate() {
            check_cancelled(cancelled)?;
            let base = 0.15 + chunk_span * index as f32;
            let _ = app.emit(
                "vox:file-progress",
                json!({ "stage": "transcribing", "fraction": base }),
            );
            let raw_text = if settings.stt_provider == "local" {
                set(app, |inner| {
                    inner.media_progress_range = Some((base, chunk_span))
                });
                let request_id = worker::next_request_id(&app.state::<Arc<worker::Worker>>());
                set(app, |inner| inner.transcribe_request_id = Some(request_id));
                let response = request_with_id(
                    app,
                    &app.state::<Arc<worker::Worker>>(),
                    json!({ "command": "transcribe", "audio_path": chunk }),
                    Duration::from_secs(60 * 60),
                    Some(request_id),
                )
                .await;
                set(app, |inner| {
                    inner.transcribe_request_id = None;
                    inner.media_progress_range = None;
                });
                response?
                    .payload
                    .get("text")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            } else {
                crate::cancellation::run(cancelled, providers::transcribe(settings, chunk)).await?
            };
            if !raw_text.trim().is_empty() {
                let text = match crate::cancellation::run(
                    cancelled,
                    providers::postprocess(settings, &raw_text),
                )
                .await
                {
                    Ok(text) => text,
                    Err(error) if error == crate::cancellation::CANCELLED => return Err(error),
                    Err(error) => {
                        warnings.push(format!(
                            "Text processing failed for part {}: {error}",
                            index + 1
                        ));
                        raw_text
                    }
                };
                text_parts.push(text);
            }
            let fraction = 0.15 + chunk_span * (index + 1) as f32;
            let _ = app.emit(
                "vox:file-progress",
                json!({ "stage": "transcribing", "fraction": fraction }),
            );
        }
        check_cancelled(cancelled)?;
        if text_parts.is_empty() {
            return Err("No speech was detected in the selected file".into());
        }

        let text = text_parts.join("\n\n");
        let mut warning = (!warnings.is_empty()).then(|| warnings.join(" "));
        if settings.history_enabled {
            match app.state::<HistoryStore>().add(
                text.clone(),
                settings.stt_provider.clone(),
                history_model.to_string(),
                Some(file_info.name.clone()),
            ) {
                Ok(entry) => {
                    let _ = app.emit("vox:history-added", entry);
                }
                Err(error) => {
                    let message = format!("The transcript could not be saved to history: {error}");
                    warning = Some(match warning {
                        Some(existing) => format!("{existing} {message}"),
                        None => message,
                    });
                }
            }
        }
        let _ = app.emit(
            "vox:file-progress",
            json!({ "stage": "done", "fraction": 1.0 }),
        );
        Ok(MediaTranscriptionResult {
            text,
            file_name: file_info.name.clone(),
            duration,
            has_video,
            warning,
        })
    }
    .await;

    media::remove_files(&temporary_files);
    result
}

fn selected_transcription_model(
    inner: &crate::state::AppStateInner,
    settings: &ProviderSettings,
) -> String {
    if settings.stt_provider == "local" {
        inner
            .current_model_id
            .as_deref()
            .and_then(|id| inner.models.iter().find(|model| model.id == id))
            .map(|model| model.name.clone())
            .unwrap_or_else(|| "Local Whisper".into())
    } else {
        settings
            .providers
            .get(&settings.stt_provider)
            .map(|config| config.stt_model.clone())
            .unwrap_or_default()
    }
}

fn check_cancelled(cancelled: &std::sync::atomic::AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::SeqCst) {
        Err(crate::cancellation::CANCELLED.into())
    } else {
        Ok(())
    }
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
        &app.state::<Arc<worker::Worker>>(),
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
            checked: true,
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
    if cfg!(target_os = "macos") {
        return Err("CUDA runtime installation is not supported on macOS".into());
    }
    if !worker::is_alive(&app) {
        return Err("Python engine is not running. Restart the engine first.".into());
    }
    if worker::is_busy(&app) {
        return Err("The Python engine is busy with another operation".into());
    }
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
            &app2.state::<Arc<worker::Worker>>(),
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
                    i.cuda_runtime.checked = true;
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

#[tauri::command(rename_all = "snake_case")]
pub async fn install_worker(app: AppHandle) -> Result<(), String> {
    log::info!("command: install_worker");
    {
        let st = app.state::<AppState>();
        let mut inner = st.lock();
        if inner.worker_install.progress.is_some() {
            return Err("Worker download already in progress".into());
        }
        inner.worker_install.progress = Some(0.0);
        inner.worker_install.error = None;
    }
    emit_status(&app);

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        match worker::install_worker(&app2).await {
            Ok(()) => {
                log::info!("worker download finished; starting engine");
                set(&app2, |i| {
                    i.worker_install.progress = None;
                    i.worker_install.error = None;
                });
                match worker::start(&app2).await {
                    Ok(()) => {
                        set(&app2, |i| {
                            i.engine_status = EngineStatus::Stopped;
                            i.engine_error = None;
                        });
                        let _ = check_cuda_runtime(app2.clone()).await;
                    }
                    Err(e) => {
                        set(&app2, |i| {
                            i.engine_status = EngineStatus::Error;
                            i.engine_error = Some(e.clone());
                            i.last_error = Some(e);
                        });
                    }
                }
            }
            Err(e) => {
                log::error!("worker download failed: {e}");
                set(&app2, |i| {
                    i.worker_install.progress = None;
                    i.worker_install.error = Some(e.clone());
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
        config.api_key_set = current
            .providers
            .get(id)
            .map(|saved| saved.api_key_set)
            .unwrap_or(false);
    }
    providers::normalize(&mut settings);
    providers::validate(&settings)?;
    if settings.stt_provider == "local" && settings.local_device != "auto" {
        let state = app.state::<AppState>();
        let inner = state.lock();
        if inner.engine_status == EngineStatus::Ready {
            if let Some(model) = inner
                .current_model_id
                .as_deref()
                .and_then(|id| inner.models.iter().find(|model| model.id == id))
            {
                if !device_matches_backend(&model.backend, &settings.local_device) {
                    return Err(format!(
                        "Device '{}' is not compatible with the active {} model; choose Auto or a compatible device",
                        settings.local_device, model.backend
                    ));
                }
            }
        }
    }
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
    let (old_device, old_stt_provider, should_reload) = {
        let st = app.state::<AppState>();
        let inner = st.lock();
        (
            inner.provider_settings.local_device.clone(),
            inner.provider_settings.stt_provider.clone(),
            inner.engine_status == EngineStatus::Ready
                && inner.current_model_id.is_some()
                && settings.stt_provider == "local",
        )
    };
    let path = app
        .state::<AppState>()
        .lock()
        .provider_settings_path
        .clone();
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
    } else if old_stt_provider == "local" && settings.stt_provider != "local" {
        // Switching from local transcription to a cloud provider: release the
        // loaded model so it does not keep holding VRAM/RAM while the cloud
        // provider is active.
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = unload_model(app_clone).await {
                log::warn!("failed to unload local model after switching to cloud: {e}");
            }
        });
    }

    Ok(settings)
}

/// Switch the speech-to-text provider (used by the system tray model menu).
pub async fn switch_stt_provider(app: AppHandle, provider_id: String) -> Result<(), String> {
    let mut settings = app.state::<AppState>().lock().provider_settings.clone();
    settings.stt_provider = provider_id;
    save_provider_settings(app, settings, HashMap::new())
        .await
        .map(|_| ())
}

fn notify_model_ready(app: &AppHandle) {
    let _ = app.emit("hotyap:model-ready", serde_json::json!({}));
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.show();
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_provider_secret(
    app: AppHandle,
    provider: String,
) -> Result<ProviderSettings, String> {
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
        (
            inner.provider_settings_path.clone(),
            inner.provider_settings.clone(),
        )
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
    if app.state::<AppState>().lock().phase != Phase::Idle {
        return Err("The model catalog can only be updated while the app is idle".into());
    }
    if !worker::is_alive(&app) {
        return Err("Python engine is not running. Restart the engine first.".into());
    }
    if worker::is_busy(&app) {
        return Err("The Python engine is busy with another operation".into());
    }

    let response = request(
        &app,
        &app.state::<Arc<worker::Worker>>(),
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

    let mut count = 0;
    let model_dir = worker::model_dir(&app);
    let state = app.state::<AppState>();
    let mut inner = state.lock();
    let curated = crate::default_models();
    for mut candidate in candidates {
        if !crate::valid_catalog_entry(&candidate) {
            log::warn!(
                "ignoring discovered model {} with unsupported backend {}",
                candidate.repo_id,
                candidate.backend
            );
            continue;
        }
        if curated
            .iter()
            .any(|model| crate::same_model(model, &candidate))
        {
            continue;
        }
        let model_path = model_dir
            .join(&candidate.id)
            .join(candidate.ct2_subdir.as_deref().unwrap_or(""));
        candidate.downloaded = crate::model_files_present(&model_path, &candidate.backend);
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
        count += 1;
    }
    crate::persist_catalog(&inner.catalog_path, &inner.models)?;
    drop(inner);
    emit_status(&app);
    Ok(count)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn download_model(app: AppHandle, model_id: String) -> Result<(), String> {
    log::info!("command: download_model {}", model_id);
    if !worker::is_alive(&app) {
        return Err("Python engine is not running. Restart the engine first.".into());
    }
    if worker::is_busy(&app) {
        return Err("The Python engine is busy with another operation".into());
    }

    let (repo_id, backend, ct2_subdir, allow_patterns, revision) = {
        let st = app.state::<AppState>();
        let inner = st.lock();
        if inner.phase != Phase::Idle {
            return Err("Models can only be downloaded while the app is idle".into());
        }
        let model = inner
            .models
            .iter()
            .find(|m| m.id == model_id)
            .ok_or_else(|| format!("Model not found: {}", model_id))?;
        if !crate::model_backend_supported(&model.backend) {
            return Err(format!(
                "The {} model backend is not supported on this platform",
                model.backend
            ));
        }
        (
            model.repo_id.clone(),
            model.backend.clone(),
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
        if inner.current_model_id.as_deref() == Some(&model_id)
            && inner.model_status == ModelStatus::Downloaded
        {
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
            &app2.state::<Arc<worker::Worker>>(),
            json!({
                "command": "download_model",
                "model_dir": dir,
                "model_id": model_id,
                "repo_id": repo_id,
                "backend": backend,
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
    if !worker::is_alive(&app) {
        return Err("Python engine is not running. Restart the engine first.".into());
    }
    if worker::is_busy(&app) {
        return Err("The Python engine is busy with another operation".into());
    }

    {
        let st = app.state::<AppState>();
        let inner = st.lock();
        if inner.phase != Phase::Idle {
            return Err("Models can only be deleted while the app is idle".into());
        }
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
            &app2.state::<Arc<worker::Worker>>(),
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
pub async fn load_model(
    app: AppHandle,
    model_id: String,
    device: Option<String>,
) -> Result<(), String> {
    log::info!("command: load_model {} device={:?}", model_id, device);

    let (backend, ct2_subdir, target_device, normalized_device) = {
        let st = app.state::<AppState>();
        let inner = st.lock();
        if inner.phase != Phase::Idle {
            return Err("Models can only be loaded while the app is idle".into());
        }
        let model = inner
            .models
            .iter()
            .find(|m| m.id == model_id)
            .ok_or_else(|| format!("Model not found: {}", model_id))?;

        if !model.downloaded {
            return Err("Model is not downloaded yet".into());
        }
        if !crate::model_backend_supported(&model.backend) {
            return Err(format!(
                "The {} model backend is not supported on this platform",
                model.backend
            ));
        }
        let explicit_device = device.is_some();
        let mut dev = device.unwrap_or_else(|| inner.provider_settings.local_device.clone());
        let normalized_device = !device_matches_backend(&model.backend, &dev);
        if normalized_device {
            if explicit_device {
                return Err(format!(
                    "Device '{dev}' is not compatible with the {} model backend",
                    model.backend
                ));
            }
            dev = "auto".into();
        }
        if inner.current_model_id.as_deref() == Some(model_id.as_str())
            && inner.engine_status == EngineStatus::Ready
            && (dev == "auto" || inner.device.as_deref() == Some(dev.as_str()))
        {
            return Ok(());
        }
        (
            model.backend.clone(),
            model.ct2_subdir.clone(),
            dev,
            normalized_device,
        )
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
        for model in &mut inner.models {
            model.loaded = false;
        }
        // Loading a local model means the user wants local transcription:
        // switch the STT provider to "local" (and persist it) so the status
        // and UI stop treating a cloud provider as the active one.
        let switched_to_local = inner.provider_settings.stt_provider != "local";
        if switched_to_local {
            inner.provider_settings.stt_provider = "local".into();
        }
        if normalized_device {
            inner.provider_settings.local_device = "auto".into();
        }
        let persist = (switched_to_local || normalized_device).then(|| {
            (
                inner.provider_settings_path.clone(),
                inner.provider_settings.clone(),
            )
        });
        drop(inner);
        if let Some((path, settings)) = persist {
            if let Err(e) = crate::providers::persist_settings(&path, &settings) {
                log::warn!("failed to persist stt_provider switch to local: {e}");
                set(&app, |inner| {
                    inner.last_warning = Some(format!(
                        "The model was selected, but the provider setting could not be saved: {e}"
                    ));
                });
            }
        }
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
            &app2.state::<Arc<worker::Worker>>(),
            json!({
                "command": "load_model",
                "model_dir": dir,
                "backend": backend,
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
                    for model in &mut i.models {
                        model.loaded = model.id == model_id;
                    }
                });
                notify_model_ready(&app2);
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
        if inner.phase != Phase::Idle {
            return Err("The model can only be stopped while the app is idle".into());
        }
        if inner.engine_status == EngineStatus::Loading {
            return Err("Wait for the model to finish loading".into());
        }
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
    let persistence_error =
        crate::storage::write_atomic(&hotkey_path, format!("{shortcut}\n").as_bytes())
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
    let (provider_settings, engine_ready) = {
        let st = app.state::<AppState>();
        let inner = st.lock();
        if inner.phase != Phase::Idle {
            return Err(format!("Cannot start recording while {:?}", inner.phase));
        }
        (
            inner.provider_settings.clone(),
            inner.engine_status == EngineStatus::Ready,
        )
    };
    if !providers::stt_ready(&provider_settings, engine_ready) {
        return Err(
            "The selected transcription provider is not ready. Open Settings to configure it."
                .into(),
        );
    }
    if providers::provider_needs_key(&provider_settings.stt_provider)
        && !providers::secret_available(&provider_settings.stt_provider)
    {
        return Err("The API key for the selected transcription provider is unavailable".into());
    }
    if provider_settings.stt_provider == "local" && worker::is_busy(&app) {
        return Err("The local engine is busy with another operation".into());
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
    {
        let st = app.state::<AppState>();
        let mut inner = st.lock();
        // Opening a device can take time. Another command may have started
        // recording or changed providers while this stream was being opened.
        if inner.phase != Phase::Idle {
            return Err(format!("Cannot start recording while {:?}", inner.phase));
        }
        if !providers::stt_ready(
            &inner.provider_settings,
            inner.engine_status == EngineStatus::Ready,
        ) {
            return Err("The selected transcription provider is no longer ready".into());
        }
        if inner.provider_settings.stt_provider == "local" && worker::is_busy(&app) {
            return Err("The local engine is busy with another operation".into());
        }
        inner.phase = Phase::Recording;
        inner.mic_name = mic_name;
        inner.recorder = Some(recorder);
        inner.last_error = None;
        inner.last_warning = None;
    }

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
                    None => break,
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
        // Taking the recorder and changing phase must be atomic: the meter
        // task and a second stop request must not observe a missing recorder.
        inner.phase = Phase::Transcribing;
        inner.transcribe_cancel.store(false, Ordering::SeqCst);
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

    // Check if cancellation was requested before we even start
    if app
        .state::<AppState>()
        .lock()
        .transcribe_cancel
        .load(Ordering::SeqCst)
    {
        log::info!("transcription cancelled before start");
        set_idle_error(&app, "Transcription cancelled".to_string());
        return Err("Transcription cancelled".to_string());
    }

    if rec_duration < 0.3 || !samples.iter().any(|sample| sample.unsigned_abs() > 64) {
        let error = "Recording was too short or silent".to_string();
        set_idle_error(&app, error.clone());
        return Err(error);
    }

    let wav_path = temp_wav_path();
    // Resampling + WAV encoding are CPU/IO bound; keep them off the async
    // runtime so a long recording cannot block other tasks.
    let write_result = tauri::async_runtime::spawn_blocking({
        let wav_path = wav_path.clone();
        move || {
            let (samples, sample_rate) = if sample_rate != 16_000 {
                (crate::audio::resample_to_16k(&samples, sample_rate), 16_000)
            } else {
                (samples, sample_rate)
            };
            write_wav(&wav_path, &samples, sample_rate)
        }
    })
    .await;
    let write_result = match write_result {
        Ok(result) => result,
        Err(e) => Err(format!("Recording task failed: {e}")),
    };
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&wav_path);
        set_idle_error(&app, error.clone());
        return Err(error);
    }

    let (settings, history_model) = {
        let state = app.state::<AppState>();
        let inner = state.lock();
        let settings = inner.provider_settings.clone();
        let model = if settings.stt_provider == "local" {
            inner
                .current_model_id
                .as_deref()
                .and_then(|id| inner.models.iter().find(|model| model.id == id))
                .map(|model| model.name.clone())
                .unwrap_or_else(|| "Local Whisper".into())
        } else {
            settings
                .providers
                .get(&settings.stt_provider)
                .map(|config| config.stt_model.clone())
                .unwrap_or_default()
        };
        (settings, model)
    };
    let local_transcription = settings.stt_provider == "local";
    let cancelled = app.state::<AppState>().lock().transcribe_cancel.clone();

    // Generate a request ID upfront for local transcription so we can store
    // it in state and cancel the pending worker request later.
    let worker_request_id: Option<u64> = if local_transcription {
        if let Some(worker_arc) = app.try_state::<Arc<worker::Worker>>() {
            let rid = worker::next_request_id(&worker_arc);
            set(&app, |i| i.transcribe_request_id = Some(rid));
            Some(rid)
        } else {
            None
        }
    } else {
        None
    };

    // Cancellation can arrive while the WAV is being encoded, before a
    // request ID exists. Recheck after publishing the ID so it cannot be lost.
    if app
        .state::<AppState>()
        .lock()
        .transcribe_cancel
        .load(Ordering::SeqCst)
    {
        let _ = std::fs::remove_file(&wav_path);
        set(&app, |i| i.transcribe_request_id = None);
        set_idle_error(&app, "Transcription cancelled".to_string());
        return Err("Transcription cancelled".to_string());
    }

    let result: Result<String, String> = if local_transcription {
        // This box's CPU can run at RTF ~15-20, so a long dictation legitimately
        // takes minutes. Size the timeout from the recorded duration with a wide margin.
        let timeout = Duration::from_secs(
            (rec_duration as u64)
                .saturating_mul(60)
                .saturating_add(300)
                .min(3600),
        );
        request_with_id(
            &app,
            &app.state::<Arc<worker::Worker>>(),
            json!({"command": "transcribe", "audio_path": wav_path}),
            timeout,
            worker_request_id,
        )
        .await
        .map(|msg| {
            let text = msg
                .payload
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            log::info!(
                "local transcription: audio={:?}s inference={:?}s rtf={:?}",
                msg.payload.get("audio_s").and_then(|value| value.as_f64()),
                msg.payload
                    .get("inference_s")
                    .and_then(|value| value.as_f64()),
                msg.payload.get("rtf").and_then(|value| value.as_f64()),
            );
            text
        })
    } else {
        let _ = app.emit(
            "vox:transcribe-progress",
            json!({ "elapsed": 0, "fraction": 0.12 }),
        );
        crate::cancellation::run(&cancelled, providers::transcribe(&settings, &wav_path)).await
    };

    let _ = std::fs::remove_file(&wav_path);

    // Clear the stored request ID now that the worker call is done
    set(&app, |i| i.transcribe_request_id = None);

    // Check if the user cancelled while we were waiting
    if app
        .state::<AppState>()
        .lock()
        .transcribe_cancel
        .load(Ordering::SeqCst)
    {
        log::info!("transcription cancelled during processing");
        set_idle_error(&app, "Transcription cancelled".to_string());
        return Err("Transcription cancelled".to_string());
    }

    match result {
        Ok(raw_text) => {
            let (text, warning) = if raw_text.trim().is_empty() {
                (raw_text, None)
            } else {
                let _ = app.emit(
                    "vox:transcribe-progress",
                    json!({ "elapsed": 0, "fraction": 0.86 }),
                );
                match crate::cancellation::run(
                    &cancelled,
                    providers::postprocess(&settings, &raw_text),
                )
                .await
                {
                    Ok(processed) => (processed, None),
                    Err(error) => {
                        log::warn!("text post-processing failed; using raw transcript: {error}");
                        (
                            raw_text,
                            Some(format!(
                                "Text processing failed; the raw transcript was copied: {error}"
                            )),
                        )
                    }
                }
            };
            // Cancellation during the optional cloud post-processing stage
            // must never publish the result or overwrite the clipboard.
            // Serialize publication with cancel_transcription's state update.
            let st = app.state::<AppState>();
            let mut inner = st.lock();
            if inner.transcribe_cancel.load(Ordering::SeqCst) {
                inner.phase = Phase::Idle;
                inner.last_error = Some("Transcription cancelled".to_string());
                inner.last_warning = None;
                drop(inner);
                emit_status(&app);
                return Err("Transcription cancelled".to_string());
            }
            if text.trim().is_empty() {
                let e = "No speech detected in the recording".to_string();
                inner.phase = Phase::Idle;
                inner.last_error = Some(e.clone());
                drop(inner);
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

            inner.phase = Phase::Idle;
            inner.last_text = Some(text.clone());
            inner.last_copied = copied;
            inner.last_error = None;
            inner.last_warning = warning;
            drop(inner);

            if settings.history_enabled {
                match app.state::<HistoryStore>().add(
                    text.clone(),
                    settings.stt_provider.clone(),
                    history_model,
                    None,
                ) {
                    Ok(entry) => {
                        let _ = app.emit("vox:history-added", entry);
                    }
                    Err(error) => {
                        log::warn!("failed to save transcription history: {error}");
                        set(&app, |inner| {
                            let history_warning = format!(
                                "The transcript was copied but could not be saved to history: {error}"
                            );
                            inner.last_warning = Some(match inner.last_warning.take() {
                                Some(existing) => format!("{existing} {history_warning}"),
                                None => history_warning,
                            });
                        });
                    }
                }
            }
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
                || e.contains("Failed to write to worker")
                || e.contains("transcription timed out");
            if local_transcription && channel_broken {
                log::warn!("worker channel broken; restarting the worker");
                let _ = worker::kill(&app).await;
                if worker::start(&app).await.is_ok() {
                    // The fresh worker has no model loaded; reload the
                    // previously loaded one so the next dictation just works.
                    let model_id = {
                        let st = app.state::<AppState>();
                        let mut inner = st.lock();
                        inner.phase = Phase::Idle;
                        inner.engine_status = EngineStatus::Stopped;
                        inner.engine_error = None;
                        inner.device = None;
                        inner.compute_type = None;
                        for model in &mut inner.models {
                            model.loaded = false;
                        }
                        inner.current_model_id.clone()
                    };
                    if let Some(model_id) = model_id {
                        if let Err(reload_error) = load_model(app.clone(), model_id, None).await {
                            log::warn!("model reload after worker recovery failed: {reload_error}");
                        }
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
    inner.ptt_generation != generation && !inner.ptt_pressed && inner.phase == Phase::Recording
}

pub async fn start_recording_after_ptt(app: AppHandle, generation: u64) -> Result<(), String> {
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
pub async fn cancel_transcription(app: AppHandle) -> Result<(), String> {
    log::info!("command: cancel_transcription");
    let (was_transcribing, request_id) = {
        let st = app.state::<AppState>();
        let mut inner = st.lock();
        if inner.phase != Phase::Transcribing {
            return Ok(());
        }
        inner.transcribe_cancel.store(true, Ordering::SeqCst);
        let rid = inner.transcribe_request_id.take();
        // Keep ownership until the transcription task has actually stopped.
        // Otherwise a new recording can reset this task's cancellation flag.
        inner.ptt_pressed = false;
        inner.ptt_generation = inner.ptt_generation.wrapping_add(1);
        (true, rid)
    };
    if was_transcribing {
        // Send cancel command to the Python worker so it stops inference
        // between segments. The acknowledgement is bounded; the original
        // transcription request retains ownership until inference stops.
        if request_id.is_some() && worker::is_alive(&app) {
            if let Some(worker_arc) = app.try_state::<Arc<worker::Worker>>() {
                let _ = worker::request_with_id(
                    &app,
                    &worker_arc,
                    json!({"command": "cancel_transcription", "request_id": request_id}),
                    Duration::from_secs(3),
                    None,
                )
                .await;
            }
        }
        emit_status(&app);
    }
    Ok(())
}

#[tauri::command]
pub async fn restart_worker(app: AppHandle) -> Result<(), String> {
    log::info!("command: restart_worker");
    if app.state::<AppState>().lock().phase != Phase::Idle {
        return Err("The engine can only be restarted while the app is idle".into());
    }
    let _ = worker::kill(&app).await;
    match worker::start(&app).await {
        Ok(()) => {
            set(&app, |i| {
                i.engine_status = EngineStatus::Stopped;
                i.engine_error = None;
                i.device = None;
                i.compute_type = None;
                for model in &mut i.models {
                    model.loaded = false;
                }
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

#[tauri::command]
pub fn set_tray_language(app: AppHandle, language: String) {
    crate::apply_tray_language(&app, language.starts_with("ru"));
}

#[tauri::command]
pub fn set_app_icon(app: AppHandle, bytes: Vec<u8>) -> Result<(), String> {
    let image =
        tauri::image::Image::from_bytes(&bytes).map_err(|e| format!("invalid icon image: {e}"))?;
    if let Some(tray) = app.tray_by_id("hotyap-tray") {
        tray.set_icon(Some(image.clone()))
            .map_err(|e| format!("tray icon update failed: {e}"))?;
    }
    for window in app.webview_windows().values() {
        window
            .set_icon(image.clone())
            .map_err(|e| format!("window icon update failed: {e}"))?;
    }
    #[cfg(windows)]
    set_taskbar_icon(&app, &bytes)?;
    Ok(())
}

/// On Windows the taskbar button's icon is bound to the window's
/// AppUserModelID (falling back to the executable's icon), not to
/// `WM_SETICON`/`ICON_BIG` (that only affects the title bar / Alt-Tab). To
/// change the taskbar icon at runtime we write a small `.ico` and point the
/// window's relaunch icon resource at it via its property store.
#[cfg(windows)]
fn set_taskbar_icon(app: &AppHandle, bytes: &[u8]) -> Result<(), String> {
    use std::io::Cursor;
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_RelaunchIconResource;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::UI::Shell::PropertiesSystem::{
        IPropertyStore, SHGetPropertyStoreForWindow,
    };

    let hwnd = app
        .get_webview_window("main")
        .and_then(|window| window.hwnd().ok())
        .ok_or_else(|| "main window handle unavailable".to_string())?;

    // Build a 256px PNG (taskbar standard large icon size) from the source.
    let decoded = image::load_from_memory(bytes).map_err(|e| format!("decode icon: {e}"))?;
    let resized = decoded.resize_exact(256, 256, image::imageops::FilterType::Lanczos3);
    let mut png = Vec::new();
    resized
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("encode icon: {e}"))?;
    let ico = png_to_ico(&png);

    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app data dir unavailable: {e}"))?;
    let _ = std::fs::create_dir_all(&data_dir);
    let ico_path = data_dir.join("taskbar-icon.ico");
    std::fs::write(&ico_path, &ico).map_err(|e| format!("write taskbar icon: {e}"))?;

    let resource = format!("{},0", ico_path.display());
    let value = PROPVARIANT::from(resource.as_str());
    let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(hwnd) }
        .map_err(|e| format!("open window property store: {e}"))?;
    unsafe {
        store
            .SetValue(&PKEY_AppUserModel_RelaunchIconResource, &value)
            .map_err(|e| format!("set relaunch icon resource: {e}"))?;
        store
            .Commit()
            .map_err(|e| format!("commit relaunch icon resource: {e}"))?;
    }
    Ok(())
}

/// Wrap a 256px PNG into a single-image ICO container (ICO supports
/// PNG-compressed entries since Vista, and 256 is the largest encodable size).
#[cfg(windows)]
fn png_to_ico(png: &[u8]) -> Vec<u8> {
    let mut ico = Vec::with_capacity(6 + 16 + png.len());
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    ico.extend_from_slice(&1u16.to_le_bytes()); // image count
    ico.push(0); // width (0 == 256)
    ico.push(0); // height (0 == 256)
    ico.push(0); // color palette
    ico.push(0); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // color planes
    ico.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    ico.extend_from_slice(&(png.len() as u32).to_le_bytes()); // bytes in resource
    ico.extend_from_slice(&22u32.to_le_bytes()); // image data offset
    ico.extend_from_slice(png);
    ico
}
