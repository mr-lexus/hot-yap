mod audio;
mod commands;
mod error;
mod providers;
mod state;
mod worker;

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use state::{emit_status, AppState, AppStateInner, ModelInfo, ModelTier};

const DEFAULT_HOTKEY: &str = "Ctrl+Shift+Space";
const HOTKEY_FILE: &str = "hotkey.txt";
const PROVIDER_SETTINGS_FILE: &str = "providers.json";

fn default_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "russian-first-int8-float16".to_string(),
            name: "Russian First · Int8 / Float16".to_string(),
            description: "Russian-specialized Whisper Turbo build. Keeps English technical terms in Latin script where supported.".to_string(),
            family: "Russian First".to_string(),
            format: "CTranslate2 · Int8 / Float16".to_string(),
            size_mb: 819,
            repo_id: "coriollon/whisper-large-v3-turbo-russian".to_string(),
            ct2_subdir: Some("ct2_int8_float16".to_string()),
            allow_patterns: Some(vec!["ct2_int8_float16/*".to_string()]),
            source_url: "https://huggingface.co/coriollon/whisper-large-v3-turbo-russian".to_string(),
            revision: Some("1158957eaa77976d79e1d2e6083a55826931c37f".to_string()),
            updated_at: Some("2026-07-19".to_string()),
            downloads: Some(1760),
            tags: vec!["RU".to_string(), "EN".to_string(), "Russian First".to_string(), "Int8".to_string(), "Float16".to_string()],
            downloaded: false,
            loaded: false,
            tier: ModelTier::Medium,
        },
        ModelInfo {
            id: "russian-first-int16".to_string(),
            name: "Russian First · Int16".to_string(),
            description: "Higher-precision Russian-first build for quality-focused setups with more memory available.".to_string(),
            family: "Russian First".to_string(),
            format: "CTranslate2 · Int16".to_string(),
            size_mb: 1629,
            repo_id: "coriollon/whisper-large-v3-turbo-russian".to_string(),
            ct2_subdir: Some("ct2-int16".to_string()),
            allow_patterns: Some(vec!["ct2-int16/*".to_string()]),
            source_url: "https://huggingface.co/coriollon/whisper-large-v3-turbo-russian".to_string(),
            revision: Some("1158957eaa77976d79e1d2e6083a55826931c37f".to_string()),
            updated_at: Some("2026-07-19".to_string()),
            downloads: Some(1760),
            tags: vec!["RU".to_string(), "EN".to_string(), "Russian First".to_string(), "Int16".to_string(), "Quality".to_string()],
            downloaded: false,
            loaded: false,
            tier: ModelTier::Heavy,
        },
        ModelInfo {
            id: "ru-en-codeswitch".to_string(),
            name: "Code Switch · Int8 / Float16".to_string(),
            description: "Fine-tuned for Russian speech mixed with English technical terms such as Python, Git, Docker and React.".to_string(),
            family: "Code Switch".to_string(),
            format: "CTranslate2 · Int8 / Float16".to_string(),
            size_mb: 819,
            repo_id: "coriollon/whisper-large-v3-turbo-russian-codeswitch".to_string(),
            ct2_subdir: Some("ct2_int8_float16".to_string()),
            allow_patterns: Some(vec!["ct2_int8_float16/*".to_string()]),
            source_url: "https://huggingface.co/coriollon/whisper-large-v3-turbo-russian-codeswitch".to_string(),
            revision: Some("bf64d2a976a268e35041f74233f889f951f0f676".to_string()),
            updated_at: Some("2026-05-15".to_string()),
            downloads: Some(29),
            tags: vec!["RU".to_string(), "EN".to_string(), "Code".to_string(), "Switch".to_string(), "Technical terms".to_string()],
            downloaded: false,
            loaded: false,
            tier: ModelTier::Medium,
        },
        ModelInfo {
            id: "roen-tiny".to_string(),
            name: "RuEn · Tiny".to_string(),
            description: "Smallest general multilingual Whisper build. Fastest option for short Russian/English dictation.".to_string(),
            family: "RuEn".to_string(),
            format: "CTranslate2 · Int8".to_string(),
            size_mb: 78,
            repo_id: "Systran/faster-whisper-tiny".to_string(),
            ct2_subdir: None,
            allow_patterns: Some(vec!["*.bin".to_string(), "*.json".to_string(), "*.txt".to_string()]),
            source_url: "https://huggingface.co/Systran/faster-whisper-tiny".to_string(),
            revision: Some("d90ca5fe260221311c53c58e660288d3deb8d356".to_string()),
            updated_at: Some("2023-11-23".to_string()),
            downloads: Some(1268922),
            tags: vec!["RU".to_string(), "EN".to_string(), "RuEn".to_string(), "Fast".to_string()],
            downloaded: false,
            loaded: false,
            tier: ModelTier::Light,
        },
        ModelInfo {
            id: "roen-base".to_string(),
            name: "RuEn · Base".to_string(),
            description: "Light multilingual build with a little more recognition quality than Tiny.".to_string(),
            family: "RuEn".to_string(),
            format: "CTranslate2 · Int8".to_string(),
            size_mb: 148,
            repo_id: "Systran/faster-whisper-base".to_string(),
            ct2_subdir: None,
            allow_patterns: Some(vec!["*.bin".to_string(), "*.json".to_string(), "*.txt".to_string()]),
            source_url: "https://huggingface.co/Systran/faster-whisper-base".to_string(),
            revision: Some("ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66".to_string()),
            updated_at: Some("2023-11-23".to_string()),
            downloads: Some(1396321),
            tags: vec!["RU".to_string(), "EN".to_string(), "RuEn".to_string(), "Fast".to_string()],
            downloaded: false,
            loaded: false,
            tier: ModelTier::Light,
        },
        ModelInfo {
            id: "roen-small".to_string(),
            name: "RuEn · Small".to_string(),
            description: "Balanced multilingual model for faster everyday dictation.".to_string(),
            family: "RuEn".to_string(),
            format: "CTranslate2 · Int8".to_string(),
            size_mb: 486,
            repo_id: "Systran/faster-whisper-small".to_string(),
            ct2_subdir: None,
            allow_patterns: Some(vec!["*.bin".to_string(), "*.json".to_string(), "*.txt".to_string()]),
            source_url: "https://huggingface.co/Systran/faster-whisper-small".to_string(),
            revision: Some("536b0662742c02347bc0e980a01041f333bce120".to_string()),
            updated_at: Some("2023-11-23".to_string()),
            downloads: Some(1921437),
            tags: vec!["RU".to_string(), "EN".to_string(), "RuEn".to_string(), "Balanced".to_string()],
            downloaded: false,
            loaded: false,
            tier: ModelTier::Medium,
        },
        ModelInfo {
            id: "roen-medium".to_string(),
            name: "RuEn · Medium".to_string(),
            description: "Higher-quality multilingual model. A practical step up from Small.".to_string(),
            family: "RuEn".to_string(),
            format: "CTranslate2 · Int8".to_string(),
            size_mb: 1531,
            repo_id: "Systran/faster-whisper-medium".to_string(),
            ct2_subdir: None,
            allow_patterns: Some(vec!["*.bin".to_string(), "*.json".to_string(), "*.txt".to_string()]),
            source_url: "https://huggingface.co/Systran/faster-whisper-medium".to_string(),
            revision: Some("08e178d48790749d25932bbc082711ddcfdfbc4f".to_string()),
            updated_at: Some("2023-11-23".to_string()),
            downloads: Some(473173),
            tags: vec!["RU".to_string(), "EN".to_string(), "RuEn".to_string(), "Quality".to_string()],
            downloaded: false,
            loaded: false,
            tier: ModelTier::Heavy,
        },
        ModelInfo {
            id: "roen-large-v3".to_string(),
            name: "RuEn · Large v3".to_string(),
            description: "Maximum general multilingual quality. Requires substantial memory and is slowest on CPU.".to_string(),
            family: "RuEn".to_string(),
            format: "CTranslate2 · Int8".to_string(),
            size_mb: 3091,
            repo_id: "Systran/faster-whisper-large-v3".to_string(),
            ct2_subdir: None,
            allow_patterns: Some(vec!["*.bin".to_string(), "*.json".to_string(), "*.txt".to_string()]),
            source_url: "https://huggingface.co/Systran/faster-whisper-large-v3".to_string(),
            revision: Some("edaa852ec7e145841d8ffdb056a99866b5f0a478".to_string()),
            updated_at: Some("2023-11-23".to_string()),
            downloads: Some(1079258),
            tags: vec!["RU".to_string(), "EN".to_string(), "RuEn".to_string(), "Large".to_string(), "Quality".to_string()],
            downloaded: false,
            loaded: false,
            tier: ModelTier::Heavy,
        },
    ]
}

fn valid_catalog_entry(model: &ModelInfo) -> bool {
    !model.id.is_empty()
        && model.id.len() <= 80
        && model.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && model.repo_id.split('/').count() == 2
        && !model.source_url.is_empty()
}

fn load_catalog(path: &Path) -> Vec<ModelInfo> {
    let mut models = default_models();
    let Ok(contents) = std::fs::read_to_string(path) else {
        return models;
    };
    let Ok(saved) = serde_json::from_str::<Vec<ModelInfo>>(&contents) else {
        log::warn!("ignoring invalid model catalog at {}", path.display());
        return models;
    };
    for mut model in saved.into_iter().filter(valid_catalog_entry) {
        // Migrate the old display spelling without changing persisted model IDs.
        model.name = model.name.replace("RoEn", "RuEn");
        model.family = model.family.replace("RoEn", "RuEn");
        for tag in &mut model.tags {
            if tag == "RoEn" {
                *tag = "RuEn".to_string();
            }
        }
        if let Some(existing) = models.iter_mut().find(|item| item.id == model.id) {
            *existing = model;
        } else {
            models.push(model);
        }
    }
    models
}

pub(crate) fn persist_catalog(path: &Path, models: &[ModelInfo]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(models).map_err(|e| format!("cannot encode model catalog: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("cannot save model catalog: {e}"))
}

#[tauri::command]
fn ping() -> String {
    "pong".into()
}

fn show_ptt_overlay(app: &AppHandle) {
    let main_is_focused = app
        .get_webview_window("main")
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(true);
    if main_is_focused {
        return;
    }
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.show();
        let _ = app.emit_to("overlay", "hotyap:overlay-open", serde_json::json!({}));
    }
}

fn notify_overlay_error(app: &AppHandle, message: &str) {
    let _ = app.emit_to(
        "overlay",
        "hotyap:overlay-error",
        serde_json::json!({ "message": message }),
    );
}

fn apply_tray_language(app: &AppHandle, is_ru: bool) {
    {
        let st = app.state::<AppState>();
        let mut inner = st.lock();
        inner.tray_is_ru = is_ru;
    }
    refresh_tray_menu(app);
}

fn provider_display_name(id: &str) -> &str {
    match id {
        "openai" => "OpenAI",
        "deepgram" => "Deepgram",
        "groq" => "Groq",
        "elevenlabs" => "ElevenLabs",
        "assemblyai" => "AssemblyAI",
        "gemini" => "Google Gemini",
        "openrouter" => "OpenRouter",
        "anthropic" => "Anthropic",
        "xai" => "xAI",
        "bedrock" => "Amazon Bedrock",
        "ollama" => "Ollama",
        "lmstudio" => "LM Studio",
        _ => id,
    }
}

/// (Re)build the tray menu: a show/hide item, a submenu listing the downloaded
/// local models and the cloud providers that have an API key (the active one is
/// checked), and a quit item. Rebuilds only when the relevant state changes.
pub(crate) fn refresh_tray_menu(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("hotyap-tray") else {
        return;
    };

    let window_visible = app
        .get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(true);

    // Snapshot the data the menu is built from.
    let (is_ru, local_models, cloud_providers, stt_provider, current_model_id) = {
        let st = app.state::<AppState>();
        let inner = st.lock();
        let local_models: Vec<(String, String, u64)> = inner
            .models
            .iter()
            .filter(|m| m.downloaded)
            .map(|m| (m.id.clone(), m.name.clone(), m.size_mb))
            .collect();
        let cloud_providers: Vec<String> = inner
            .provider_settings
            .providers
            .iter()
            .filter(|(id, config)| {
                config.api_key_set
                    && crate::providers::STT_PROVIDER_IDS.contains(&id.as_str())
                    && id.as_str() != "local"
            })
            .map(|(id, _)| id.clone())
            .collect();
        (
            inner.tray_is_ru,
            local_models,
            cloud_providers,
            inner.provider_settings.stt_provider.clone(),
            inner.current_model_id.clone(),
        )
    };

    // Cheap signature so status broadcasts don't rebuild the menu every tick.
    let mut signature = String::new();
    signature.push_str(&format!("ru:{is_ru};vis:{window_visible};"));
    for (id, name, size) in &local_models {
        signature.push_str(&format!("m:{id}:{name}:{size};"));
    }
    for id in &cloud_providers {
        signature.push_str(&format!("p:{id};"));
    }
    signature.push_str(&format!(
        "active:{stt_provider};cur:{}",
        current_model_id.as_deref().unwrap_or("")
    ));

    {
        let st = app.state::<AppState>();
        let mut inner = st.lock();
        if inner.tray_menu_signature == signature {
            return;
        }
        inner.tray_menu_signature = signature;
    }

    let (show_text, hide_text, quit_text, model_label) = if is_ru {
        ("Показать окно", "Скрыть окно", "Выход", "Модель")
    } else {
        ("Show window", "Hide window", "Quit", "Model")
    };

    let mut submenu = SubmenuBuilder::new(app, model_label);
    for (id, name, size) in &local_models {
        let label = if *size >= 1000 {
            format!("{name} ({:.1} GB)", *size as f32 / 1000.0)
        } else {
            format!("{name} ({size} MB)")
        };
        let checked = stt_provider == "local" && current_model_id.as_deref() == Some(id.as_str());
        if let Ok(item) = CheckMenuItemBuilder::with_id(format!("model:{id}"), label)
            .checked(checked)
            .build(app)
        {
            submenu = submenu.item(&item);
        }
    }
    if !local_models.is_empty() && !cloud_providers.is_empty() {
        submenu = submenu.separator();
    }
    for id in &cloud_providers {
        if let Ok(item) = CheckMenuItemBuilder::with_id(
            format!("provider:{id}"),
            provider_display_name(id),
        )
        .checked(stt_provider == *id)
        .build(app)
        {
            submenu = submenu.item(&item);
        }
    }

    let mut menu = MenuBuilder::new(app);
    if let Ok(item) = MenuItemBuilder::with_id(
        "show",
        if window_visible { hide_text } else { show_text },
    )
    .build(app)
    {
        menu = menu.item(&item);
    }
    if !local_models.is_empty() || !cloud_providers.is_empty() {
        if let Ok(submenu) = submenu.build() {
            menu = menu.item(&submenu);
        }
    }
    if let Ok(item) = MenuItemBuilder::with_id("quit", quit_text).build(app) {
        menu = menu.item(&item);
    }
    if let Ok(menu) = menu.build() {
        let _ = tray.set_menu(Some(menu));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    let app = app.clone();
                    if event.state() == ShortcutState::Pressed {
                        match commands::mark_ptt_pressed(&app) {
                            Ok(Some(generation)) => {
                                show_ptt_overlay(&app);
                                tauri::async_runtime::spawn(async move {
                                    if let Err(e) =
                                        commands::start_recording_after_ptt(app.clone(), generation).await
                                    {
                                        log::info!("push-to-talk press failed: {e}");
                                        notify_overlay_error(&app, &e);
                                    }
                                });
                            }
                            Ok(None) => {}
                            Err(e) => log::info!("push-to-talk press failed: {e}"),
                        }
                    } else if commands::mark_ptt_released(&app).is_some() {
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = commands::stop_recording_after_ptt(app.clone()).await {
                                log::info!("push-to-talk release failed: {e}");
                                notify_overlay_error(&app, &e);
                            }
                        });
                    }
                })
                .build(),
        )
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))?;
                window.set_icon(icon)?;
            }

            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("cannot resolve app data dir: {e}"))?;
            let model_dir = data_dir.join("models");
            let catalog_path = data_dir.join("catalog.json");
            let hotkey_path = data_dir.join(HOTKEY_FILE);
            let provider_settings_path = data_dir.join(PROVIDER_SETTINGS_FILE);
            let provider_settings = providers::load_settings(&provider_settings_path);
            let configured_hotkey = std::fs::read_to_string(&hotkey_path)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| DEFAULT_HOTKEY.to_string());
            std::fs::create_dir_all(&model_dir)
                .map_err(|e| format!("cannot create model dir {}: {e}", model_dir.display()))?;

            // Check which models are already downloaded
            let mut models_with_status = load_catalog(&catalog_path);
            for model in &mut models_with_status {
                let model_path = model_dir.join(&model.id).join(model.ct2_subdir.as_deref().unwrap_or(""));
                model.downloaded = model_path.join("model.bin").exists();
            }

            app.manage(AppState(Mutex::new(AppStateInner {
                model_status: state::ModelStatus::NotDownloaded,
                model_error: None,
                engine_status: state::EngineStatus::Stopped,
                engine_error: None,
                device: None,
                compute_type: None,
                phase: state::Phase::Idle,
                mic_name: None,
                hotkey: configured_hotkey.clone(),
                hotkey_registered: false,
                hotkey_warning: None,
                last_text: None,
                last_copied: false,
                last_error: None,
                last_warning: None,
                model_dir,
                catalog_path,
                recorder: None,
                models: models_with_status.clone(),
                current_model_id: None,
                model_progress: None,
                transcribe_progress: None,
                transcribe_elapsed: 0.0,
                audio_level: 0.0,
                audio_spectrum: vec![0.0; 32],
                ptt_pressed: false,
                ptt_generation: 0,
                hotkey_path,
                provider_settings_path,
                provider_settings,
                cuda_runtime: state::CudaRuntimeReport::default(),
                closing: false,
                force_quit: false,
                tray_is_ru: false,
                tray_menu_signature: String::new(),
                transcribe_cancel: Arc::new(AtomicBool::new(false)),
                transcribe_request_id: None,
            })));

            // Start the Python worker asynchronously; the app still opens if it fails.
            let app2 = app.handle().clone();
            let model_dir = app.state::<AppState>().lock().model_dir.clone();
            let catalog = models_with_status;
            tauri::async_runtime::spawn(async move {
                if let Err(e) = worker::start(&app2).await {
                    log::error!("worker start failed: {e}");
                    {
                        let st = app2.state::<AppState>();
                        let mut inner = st.lock();
                        inner.engine_status = state::EngineStatus::Error;
                        inner.engine_error = Some(e);
                    }
                } else {
                    // Refresh model status from disk.
                    if let Ok(msg) = worker::request(
                        &app2,
                        &*app2.state::<Arc<worker::Worker>>(),
                        serde_json::json!({"command": "status", "model_dir": model_dir, "catalog": catalog}),
                        Duration::from_secs(15),
                    )
                    .await
                    {
                        let model_statuses: Vec<serde_json::Value> = msg
                            .payload
                            .get("models")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();

                        let st = app2.state::<AppState>();
                        let mut inner = st.lock();
                        for ms in model_statuses {
                            if let (Some(id), Some(downloaded)) = (
                                ms.get("id").and_then(|v| v.as_str()),
                                ms.get("downloaded").and_then(|v| v.as_bool()),
                            ) {
                                if let Some(m) = inner.models.iter_mut().find(|m| m.id == id) {
                                    m.downloaded = downloaded;
                                }
                            }
                        }

                        // Set initial model status based on first downloaded model
                        if let Some(downloaded_model) = inner.models.iter().find(|m| m.downloaded) {
                            inner.current_model_id = Some(downloaded_model.id.clone());
                            inner.model_status = state::ModelStatus::Downloaded;
                        }
                        drop(inner);
                    }
                    // Surface whether the CUDA runtime is usable (banner in the UI).
                    let _ = commands::check_cuda_runtime(app2.clone()).await;
                }
                emit_status(&app2);
            });
            emit_status(&app.handle());

            // Register the configured global hotkey; failure must not kill the app.
            match app
                .handle()
                .global_shortcut()
                .register(configured_hotkey.as_str())
            {
                Ok(_) => {
                    log::info!("global shortcut registered: {configured_hotkey}");
                    {
                        let st = app.state::<AppState>();
                        let mut inner = st.lock();
                        inner.hotkey_registered = true;
                    }
                }
                Err(e) => {
                    log::warn!("global shortcut registration failed: {e}");
                    let st = app.state::<AppState>();
                    let mut inner = st.lock();
                    inner.hotkey_registered = false;
                    inner.hotkey_warning =
                        Some(format!("Could not register {configured_hotkey}: {e}"));
                }
            }
            emit_status(&app.handle());

            // System tray icon — detect system locale for initial text.
            let is_ru = std::env::var("LANG")
                .map(|v| v.to_lowercase().starts_with("ru"))
                .unwrap_or(false);
            {
                let st = app.state::<AppState>();
                let mut inner = st.lock();
                inner.tray_is_ru = is_ru;
            }
            let _tray = TrayIconBuilder::with_id("hotyap-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("HotYap")
                .on_menu_event(move |app, event| {
                    let id = event.id.as_ref();
                    if let Some(model_id) = id.strip_prefix("model:") {
                        let app = app.clone();
                        let model_id = model_id.to_string();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = commands::load_model(app, model_id, None).await {
                                log::warn!("tray model switch failed: {e}");
                            }
                        });
                        return;
                    }
                    if let Some(provider_id) = id.strip_prefix("provider:") {
                        let app = app.clone();
                        let provider_id = provider_id.to_string();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = commands::switch_stt_provider(app, provider_id).await {
                                log::warn!("tray provider switch failed: {e}");
                            }
                        });
                        return;
                    }
                    match id {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                if window.is_visible().unwrap_or(false) {
                                    let _ = window.hide();
                                } else {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                            refresh_tray_menu(app);
                        }
                        "quit" => {
                            {
                                let st = app.state::<AppState>();
                                let mut inner = st.lock();
                                inner.force_quit = true;
                                inner.closing = true;
                            }
                            let app = app.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = tokio::time::timeout(
                                    Duration::from_secs(15),
                                    worker::shutdown(&app),
                                )
                                .await;
                                app.exit(0);
                            });
                        }
                        _ => {}
                    }
                })
                .build(app)?;
            refresh_tray_menu(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            commands::get_status,
            commands::list_models,
            commands::download_model,
            commands::delete_model,
            commands::load_model,
            commands::unload_model,
            commands::start_recording,
            commands::stop_recording,
            commands::press_to_talk,
            commands::release_to_talk,
            commands::set_hotkey,
            commands::restart_worker,
            commands::update_model_catalog,
            commands::check_cuda_runtime,
            commands::install_cuda_runtime,
            commands::get_provider_settings,
            commands::save_provider_settings,
            commands::delete_provider_secret,
            commands::cancel_transcription,
            commands::set_tray_language,
            commands::set_app_icon
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle().clone();
                // If the user explicitly requested quit from the tray, allow
                // the window to close and the app to shut down normally.
                let force_quit = {
                    let st = app.state::<AppState>();
                    let inner = st.lock();
                    inner.force_quit
                };
                if force_quit {
                    // Start graceful shutdown (worker teardown + exit) only once.
                    let already_closing = {
                        let st = app.state::<AppState>();
                        let mut inner = st.lock();
                        if inner.closing {
                            true
                        } else {
                            inner.closing = true;
                            false
                        }
                    };
                    if already_closing {
                        return;
                    }
                    api.prevent_close();
                    tauri::async_runtime::spawn(async move {
                        let _ =
                            tokio::time::timeout(Duration::from_secs(15), worker::shutdown(&app))
                                .await;
                        app.exit(0);
                    });
                } else {
                    // Minimize to system tray instead of closing.
                    api.prevent_close();
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
