use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    NotDownloaded,
    Downloading,
    Downloaded,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineStatus {
    Stopped,
    Loading,
    Ready,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Recording,
    Transcribing,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Light,
    Medium,
    Heavy,
}

/// Availability of the NVIDIA cuBLAS runtime for GPU inference.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct CudaRuntimeReport {
    /// Whether the NVIDIA driver (nvcuda.dll) is present and a GPU is usable.
    pub gpu_available: bool,
    /// Whether every required cuBLAS DLL could be loaded.
    pub runtime_ok: bool,
    /// Names of the DLLs that failed to load.
    pub missing: Vec<String>,
    /// Non-null while a runtime download is in progress (0.0..1.0).
    pub progress: Option<f32>,
    /// Human-readable error from the last failed attempt, if any.
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub family: String,
    pub format: String,
    pub size_mb: u64,
    pub repo_id: String,
    pub ct2_subdir: Option<String>,
    pub allow_patterns: Option<Vec<String>>,
    pub source_url: String,
    pub revision: Option<String>,
    pub updated_at: Option<String>,
    pub downloads: Option<u64>,
    pub tags: Vec<String>,
    pub downloaded: bool,
    pub loaded: bool,
    pub tier: ModelTier,
}

#[derive(Clone, Debug, Serialize)]
pub struct StatusReport {
    pub model_status: ModelStatus,
    pub model_error: Option<String>,
    pub engine_status: EngineStatus,
    pub engine_error: Option<String>,
    pub device: Option<String>,
    pub compute_type: Option<String>,
    pub phase: Phase,
    pub mic_name: Option<String>,
    pub hotkey: String,
    pub hotkey_registered: bool,
    pub hotkey_warning: Option<String>,
    pub last_text: Option<String>,
    pub last_copied: bool,
    pub worker_alive: bool,
    pub last_error: Option<String>,
    pub last_warning: Option<String>,
    pub models: Vec<ModelInfo>,
    pub current_model_id: Option<String>,
    pub model_progress: Option<f32>,
    pub transcribe_progress: Option<f32>,
    pub transcribe_elapsed: f32,
    pub audio_level: f32,
    pub audio_spectrum: Vec<f32>,
    pub stt_provider: String,
    pub stt_model: String,
    pub stt_ready: bool,
    pub text_provider: String,
    pub local_device: String,
    pub cuda_runtime: CudaRuntimeReport,
    pub provider_settings: crate::providers::ProviderSettings,
}

pub struct AppStateInner {
    pub model_status: ModelStatus,
    pub model_error: Option<String>,
    pub engine_status: EngineStatus,
    pub engine_error: Option<String>,
    pub device: Option<String>,
    pub compute_type: Option<String>,
    pub phase: Phase,
    pub mic_name: Option<String>,
    pub hotkey: String,
    pub hotkey_registered: bool,
    pub hotkey_warning: Option<String>,
    pub last_text: Option<String>,
    pub last_copied: bool,
    pub last_error: Option<String>,
    pub last_warning: Option<String>,
    pub model_dir: PathBuf,
    pub catalog_path: PathBuf,
    pub recorder: Option<crate::audio::Recorder>,
    pub models: Vec<ModelInfo>,
    pub current_model_id: Option<String>,
    pub model_progress: Option<f32>,
    pub transcribe_progress: Option<f32>,
    pub transcribe_elapsed: f32,
    pub audio_level: f32,
    pub audio_spectrum: Vec<f32>,
    pub ptt_pressed: bool,
    pub ptt_generation: u64,
    pub hotkey_path: PathBuf,
    pub provider_settings_path: PathBuf,
    pub provider_settings: crate::providers::ProviderSettings,
    pub cuda_runtime: CudaRuntimeReport,
    /// Set once the first window close starts worker teardown; guards against
    /// re-entrant CloseRequested events during app exit.
    pub closing: bool,
    /// Set to true when the user requests cancellation of an in-progress transcription.
    pub transcribe_cancel: Arc<AtomicBool>,
    /// The worker request ID of the in-progress transcription (used to cancel the pending channel).
    pub transcribe_request_id: Option<u64>,
}

pub struct AppState(pub Mutex<AppStateInner>);

impl AppState {
    pub fn lock(&self) -> std::sync::MutexGuard<'_, AppStateInner> {
        self.0.lock().unwrap()
    }
}

impl AppStateInner {
    pub fn report(&self, worker_alive: bool) -> StatusReport {
        let stt_model = if self.provider_settings.stt_provider == "local" {
            self.current_model_id
                .as_ref()
                .and_then(|id| self.models.iter().find(|model| &model.id == id))
                .map(|model| model.name.clone())
                .unwrap_or_default()
        } else {
            self.provider_settings
                .providers
                .get(&self.provider_settings.stt_provider)
                .map(|config| config.stt_model.clone())
                .unwrap_or_default()
        };
        let stt_ready = crate::providers::stt_ready(
            &self.provider_settings,
            self.engine_status == EngineStatus::Ready,
        );
        StatusReport {
            model_status: self.model_status,
            model_error: self.model_error.clone(),
            engine_status: self.engine_status,
            engine_error: self.engine_error.clone(),
            device: self.device.clone(),
            compute_type: self.compute_type.clone(),
            phase: self.phase,
            mic_name: self.mic_name.clone(),
            hotkey: self.hotkey.clone(),
            hotkey_registered: self.hotkey_registered,
            hotkey_warning: self.hotkey_warning.clone(),
            last_text: self.last_text.clone(),
            last_copied: self.last_copied,
            worker_alive,
            last_error: self.last_error.clone(),
            last_warning: self.last_warning.clone(),
            models: self.models.clone(),
            current_model_id: self.current_model_id.clone(),
            model_progress: self.model_progress,
            transcribe_progress: self.transcribe_progress,
            transcribe_elapsed: self.transcribe_elapsed,
            audio_level: self.audio_level,
            audio_spectrum: self.audio_spectrum.clone(),
            stt_provider: self.provider_settings.stt_provider.clone(),
            stt_model,
            stt_ready,
            text_provider: self.provider_settings.text_provider.clone(),
            local_device: self.provider_settings.local_device.clone(),
            cuda_runtime: self.cuda_runtime.clone(),
            provider_settings: self.provider_settings.clone(),
        }
    }
}

/// Lock the app state, build a StatusReport and emit it as a `vox:status` event.
pub fn emit_status(app: &AppHandle) {
    let st = app.state::<AppState>();
    let report = st.lock().report(crate::worker::is_alive(app));
    let _ = app.emit("vox:status", report);
}

/// The worker died: reset runtime state so the UI can offer a restart.
pub fn on_worker_exit(app: &AppHandle) {
    {
        let st = app.state::<AppState>();
        let mut inner = st.lock();
        inner.phase = Phase::Idle;
        if matches!(inner.engine_status, EngineStatus::Loading | EngineStatus::Ready) {
            inner.engine_status = EngineStatus::Error;
            inner.engine_error = Some(
                "Python engine stopped unexpectedly. Use 'Restart engine'.".to_string(),
            );
        }
        inner.recorder = None;
        inner.ptt_pressed = false;
        inner.ptt_generation = inner.ptt_generation.wrapping_add(1);
    }
    emit_status(app);
}
