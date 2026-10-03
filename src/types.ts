export type ModelStatus = "not_downloaded" | "downloading" | "downloaded" | "error";
export type EngineStatus = "stopped" | "loading" | "ready" | "error" | "not_installed";
export type Phase = "idle" | "recording" | "transcribing";

export type ModelTier = "light" | "medium" | "heavy";
export type ModelBackend = "ctranslate2" | "mlx";

export interface ModelInfo {
  id: string;
  name: string;
  description: string;
  family: string;
  format: string;
  size_mb: number;
  repo_id: string;
  backend: ModelBackend;
  ct2_subdir?: string;
  allow_patterns?: string[];
  source_url: string;
  revision?: string;
  updated_at?: string;
  downloads?: number;
  tags: string[];
  downloaded: boolean;
  loaded: boolean;
  tier: ModelTier;
}

export interface CudaRuntimeReport {
  checked: boolean;
  gpu_available: boolean;
  runtime_ok: boolean;
  missing: string[];
  progress: number | null;
  error: string | null;
}

export interface WorkerInstallReport {
  progress: number | null;
  error: string | null;
}

export interface StatusReport {
  live_text: string;
  last_pasted: boolean;
  model_status: ModelStatus;
  model_error: string | null;
  engine_status: EngineStatus;
  engine_error: string | null;
  device: string | null;
  compute_type: string | null;
  phase: Phase;
  mic_name: string | null;
  hotkey: string;
  hotkey_registered: boolean;
  hotkey_warning: string | null;
  last_text: string | null;
  last_copied: boolean;
  worker_alive: boolean;
  last_error: string | null;
  last_warning: string | null;
  models: ModelInfo[];
  current_model_id: string | null;
  model_progress: number | null;
  transcribe_progress: number | null;
  transcribe_elapsed: number;
  audio_level: number;
  audio_spectrum: number[];
  stt_provider: string;
  stt_model: string;
  stt_ready: boolean;
  text_provider: string;
  local_device: string;
  cuda_runtime: CudaRuntimeReport;
  worker_install: WorkerInstallReport;
  cuda_supported: boolean;
  metal_supported: boolean;
  provider_settings: ProviderSettings;
}

export interface ProviderConfig {
  endpoint: string;
  stt_model: string;
  text_model: string;
  api_key_set: boolean;
}

export interface ProviderSettings {
  live_transcription: boolean;
  live_final_pass: boolean;
  dictation_preview: boolean;
  input_device: string | null;
  system_audio: "nothing" | "mute" | "duck";
  launch_at_startup: boolean;
  close_to_tray: boolean;
  auto_paste: boolean;
  paste_terminal: boolean;
  paste_delay_ms: number;
  stt_provider: string;
  text_provider: string;
  postprocess_prompt: string;
  history_enabled: boolean;
  local_device: string;
  providers: Record<string, ProviderConfig>;
}

export interface DictionaryEntry {
  id: string;
  heard: string;
  written: string;
  project_id: string | null;
  origin: "manual" | "learned" | "project";
  enabled: boolean;
}
export interface DictionaryProject { id: string; name: string; path: string }
export interface DictionaryData {
  revision: number;
  entries: DictionaryEntry[];
  suggestions: DictionaryEntry[];
  projects: DictionaryProject[];
  active_project: string | null;
  learning: "off" | "suggest" | "auto";
}
export interface PasteSupport { platform: string; available: boolean; reason: string }

export interface HistoryEntry {
  id: string;
  text: string;
  created_at: number;
  favorite: boolean;
  provider: string;
  model: string;
  source_name?: string | null;
}

export interface MediaFileInfo {
  name: string;
  extension: string;
  size_bytes: number;
  is_video: boolean;
}

export interface MediaTranscriptionResult {
  text: string;
  file_name: string;
  duration: number;
  has_video: boolean;
  warning: string | null;
}

export const MODEL_LABEL: Record<ModelStatus, string> = {
  not_downloaded: "Not downloaded",
  downloading: "Downloading…",
  downloaded: "Downloaded",
  error: "Error",
};

export const ENGINE_LABEL: Record<EngineStatus, string> = {
  stopped: "Stopped",
  loading: "Loading…",
  ready: "Ready",
  error: "Error",
  not_installed: "Not installed",
};

export const TIER_LABEL: Record<ModelTier, string> = {
  light: "Light",
  medium: "Medium",
  heavy: "Heavy",
};
