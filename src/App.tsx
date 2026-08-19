import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import { type CudaRuntimeReport, type ProviderSettings, type StatusReport } from "./types";
import ModelManager from "./ModelManager";
import BrandLogo from "./BrandLogo";
import Icon from "./Icons";
import TitleBar from "./TitleBar";
import SpectrumAnalyzer from "./SpectrumAnalyzer";
import TranscriptionProgress from "./TranscriptionProgress";
import SettingsModal, { providerName } from "./SettingsModal";
import { applyAppearance, applyIconPreference, savedAccent, savedIconPreference, savedTheme, type Accent, type IconPreference, type Theme } from "./appearance";
import "./app.css";

const DEFAULT_STATUS: StatusReport = {
  model_status: "not_downloaded",
  model_error: null,
  engine_status: "stopped",
  engine_error: null,
  device: null,
  compute_type: null,
  phase: "idle",
  mic_name: null,
  hotkey: "Ctrl+Shift+Space",
  hotkey_registered: false,
  hotkey_warning: null,
  last_text: null,
  last_copied: false,
  worker_alive: true,
  last_error: null,
  last_warning: null,
  models: [],
  current_model_id: null,
  model_progress: null,
  transcribe_progress: null,
  transcribe_elapsed: 0,
  audio_level: 0,
  audio_spectrum: [],
  stt_provider: "local",
  stt_model: "",
  stt_ready: false,
  text_provider: "none",
  local_device: "auto",
  cuda_runtime: {
    gpu_available: false,
    runtime_ok: true,
    missing: [],
    progress: null,
    error: null,
  },
  provider_settings: {
    stt_provider: "local",
    text_provider: "none",
    postprocess_prompt: "",
    local_device: "auto",
    providers: {},
  },
};

function hotkeyParts(shortcut: string): string[] {
  return shortcut.split("+").filter(Boolean).map((part) => {
    if (part.startsWith("Key")) return part.slice(3);
    if (part.startsWith("Digit")) return part.slice(5);
    return part;
  });
}

function shortcutFromKeyEvent(event: ReactKeyboardEvent<HTMLButtonElement>): string | null {
  if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) return null;
  if (!event.code || event.code === "Unidentified") return null;

  const modifiers = [
    event.ctrlKey ? "Ctrl" : "",
    event.altKey ? "Alt" : "",
    event.shiftKey ? "Shift" : "",
    event.metaKey ? "Super" : "",
  ].filter(Boolean);
  return [...modifiers, event.code].join("+");
}

export default function App() {
  const { t, i18n } = useTranslation();
  const [status, setStatus] = useState<StatusReport>(DEFAULT_STATUS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [capturingHotkey, setCapturingHotkey] = useState(false);
  const [holdingToTalk, setHoldingToTalk] = useState(false);
  const [releasingToTalk, setReleasingToTalk] = useState(false);
  const [modelManagerOpen, setModelManagerOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [theme, setTheme] = useState<Theme>(savedTheme);
  const [accent, setAccent] = useState<Accent>(savedAccent);
  const [iconPreference, setIconPreference] = useState<IconPreference>(savedIconPreference);
  const listenersRef = useRef<UnlistenFn[]>([]);
  const holdingToTalkRef = useRef(false);
  const releaseCommandDoneRef = useRef(false);
  const statusRef = useRef(status);
  statusRef.current = status;

  useEffect(() => {
    applyAppearance(theme, accent);
    window.localStorage.setItem("hotyap-theme", theme);
    window.localStorage.setItem("hotyap-accent", accent);
    void emit("hotyap:appearance", { theme, accent });
  }, [theme, accent]);

  useEffect(() => {
    window.localStorage.setItem("hotyap-icon-preference", iconPreference);
    void applyIconPreference(iconPreference).catch((value) => console.error("icon update failed:", value));
    void emit("hotyap:icon-preference", { preference: iconPreference });

    if (iconPreference !== "system") return;
    let unlisten: UnlistenFn | undefined;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const refreshIcon = () => void applyIconPreference("system").catch(() => {});
    media.addEventListener("change", refreshIcon);
    void getCurrentWindow().onThemeChanged(refreshIcon).then((cleanup) => { unlisten = cleanup; });
    return () => {
      media.removeEventListener("change", refreshIcon);
      unlisten?.();
    };
  }, [iconPreference]);

  useEffect(() => {
    const syncLanguage = (language = i18n.resolvedLanguage ?? i18n.language) => {
      const locale = language.startsWith("ru") ? "ru" : "en";
      const fixedT = i18n.getFixedT(locale);
      void getCurrentWindow().setTitle(fixedT("brand.windowTitle"));
      void emit("hotyap:language", { language: locale });
    };
    const syncCurrentLanguage = () => syncLanguage();
    syncCurrentLanguage();
    i18n.on("initialized", syncCurrentLanguage);
    i18n.on("languageChanged", syncLanguage);
    return () => {
      i18n.off("initialized", syncCurrentLanguage);
      i18n.off("languageChanged", syncLanguage);
    };
  }, [i18n]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const s = await invoke<StatusReport>("get_status");
        if (!cancelled) setStatus(s);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
      if (cancelled) return;
      const un1 = await listen<StatusReport>("vox:status", (e) => {
        setStatus(e.payload);
      });
       const un2 = await listen<{ fraction: number }>("vox:download-progress", (e) => {
         setStatus(prev => ({ ...prev, model_progress: e.payload.fraction }));
       });
       const un3 = await listen<{ elapsed: number; fraction?: number }>("vox:transcribe-progress", (e) => {
         setStatus(prev => ({
           ...prev,
           transcribe_progress: e.payload.fraction ?? Math.min(1, e.payload.elapsed / 30),
           transcribe_elapsed: Math.round(e.payload.elapsed),
         }));
       });
       const un4 = await listen<{ level: number; spectrum: number[] }>("vox:audio-meter", (e) => {
         setStatus(prev => ({
           ...prev,
           audio_level: e.payload.level,
           audio_spectrum: e.payload.spectrum,
         }));
       });
       const un5 = await listen<{ fraction: number }>("vox:cuda-runtime-progress", (e) => {
         setStatus(prev => ({
           ...prev,
           cuda_runtime: {
             ...prev.cuda_runtime,
             progress: e.payload.fraction,
             error: null,
           },
         }));
       });
       listenersRef.current = [un1, un2, un3, un4, un5];
       // Ask the backend whether the CUDA runtime is usable; the result
       // decides whether the "download CUDA runtime" banner is shown.
       invoke<CudaRuntimeReport>("check_cuda_runtime")
         .then((report) => setStatus(prev => ({ ...prev, cuda_runtime: report })))
         .catch((e) => console.error("check_cuda_runtime failed:", e));
    })();
    const poll = setInterval(() => {
      invoke<StatusReport>("get_status")
        .then((s) => setStatus(s))
        .catch(() => {});
    }, 5000);
    return () => {
      cancelled = true;
      clearInterval(poll);
      listenersRef.current.forEach((un) => un());
    };
  }, []);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const recording = status.phase === "recording" && !releasingToTalk;
  const transcribing = status.phase === "transcribing" || releasingToTalk;
  const canRecord =
    status.stt_ready && status.phase === "idle" && !busy;

  const currentModel = status.models.find(m => m.id === status.current_model_id);
  const cloudTranscription = status.stt_provider !== "local";
  const activeTranscriber = cloudTranscription
    ? `${providerName(status.stt_provider)}${status.stt_model ? ` В· ${status.stt_model}` : ""}`
    : currentModel?.name ?? t("model.none");
  const displayedEngineStatus = status.stt_ready ? "ready" : cloudTranscription ? "stopped" : status.engine_status;
  const hasDownloadedModel = status.models.some((model) => model.downloaded);
  const modelProgress = status.model_progress == null ? null : Math.round(status.model_progress * 100);
  const modelActivity = status.model_status === "downloading"
    ? modelProgress == null ? t("model.downloadingUnknown") : t("model.downloading", { progress: modelProgress })
    : status.model_status === "error"
      ? status.model_error ?? t("models.downloadFailed")
      : currentModel?.downloaded
        ? t("model.downloaded")
        : hasDownloadedModel
          ? t("model.selectDownloaded")
          : t("model.noneDownloaded");

  const pressToTalk = () => {
    if (holdingToTalkRef.current || !canRecord) return;
    holdingToTalkRef.current = true;
    setHoldingToTalk(true);
    setReleasingToTalk(false);
    releaseCommandDoneRef.current = false;
    setError(null);
    invoke("press_to_talk").catch((e) => {
      holdingToTalkRef.current = false;
      setHoldingToTalk(false);
      setError(String(e));
    });
  };

  const releaseToTalk = () => {
    if (!holdingToTalkRef.current) return;
    holdingToTalkRef.current = false;
    setHoldingToTalk(false);
    setReleasingToTalk(true);
    releaseCommandDoneRef.current = false;
    invoke("release_to_talk")
      .then(() => {
        releaseCommandDoneRef.current = true;
        if (statusRef.current.phase !== "recording") setReleasingToTalk(false);
      })
      .catch((e) => {
        releaseCommandDoneRef.current = false;
        setReleasingToTalk(false);
        setError(String(e));
      });
  };

  const cancelTranscription = () => {
    invoke("cancel_transcription").catch((e) => setError(String(e)));
  };

  useEffect(() => {
    if (releaseCommandDoneRef.current && status.phase !== "recording") {
      releaseCommandDoneRef.current = false;
      setReleasingToTalk(false);
    }
  }, [status.phase]);

  useEffect(() => {
    const releaseOnBlur = () => {
      if (!holdingToTalkRef.current) return;
      holdingToTalkRef.current = false;
      setHoldingToTalk(false);
      setReleasingToTalk(true);
      releaseCommandDoneRef.current = false;
      invoke("release_to_talk")
        .then(() => {
          releaseCommandDoneRef.current = true;
          if (statusRef.current.phase !== "recording") setReleasingToTalk(false);
        })
        .catch((e) => {
          releaseCommandDoneRef.current = false;
          setReleasingToTalk(false);
          setError(String(e));
        });
    };
    window.addEventListener("blur", releaseOnBlur);
    return () => {
      window.removeEventListener("blur", releaseOnBlur);
      releaseOnBlur();
    };
  }, []);

  const captureHotkey = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (event.repeat) return;
    event.preventDefault();
    const shortcut = shortcutFromKeyEvent(event);
    if (!shortcut) return;
    setCapturingHotkey(false);
    void run(() => invoke("set_hotkey", { shortcut }));
  };

  return (
    <>
      <TitleBar
        theme={theme}
        status={status}
        onModelSwitch={async (target) => {
          setBusy(true);
          setError(null);
          try {
            if (target.startsWith("local:")) {
              const modelId = target.slice(6);
              await invoke("load_model", { model_id: modelId });
            } else if (target.startsWith("cloud:")) {
              const providerId = target.slice(6);
              const settings = await invoke<ProviderSettings>("get_provider_settings");
              await invoke("save_provider_settings", {
                settings: { ...settings, stt_provider: providerId },
                secrets: {},
              });
            }
          } catch (e) {
            setError(String(e));
          } finally {
            setBusy(false);
          }
        }}
      />
      <div className="app">
        <header className="topbar">
        <div className="brand-lockup">
          <div className="brand-mark"><BrandLogo size={50} variant={theme === "dark" ? "light" : "dark"} /></div>
          <div>
            <p className="eyebrow">{t("brand.kicker")}</p>
            <h1>{t("brand.name")}</h1>
            <p className="subtitle">{t("brand.tagline")}</p>
          </div>
        </div>
        <div className="topbar-actions">
          <div className="locale-switch" aria-label={t("theme.language")}>
            <button className={i18n.language.startsWith("en") ? "active" : ""} onClick={() => void i18n.changeLanguage("en")}>EN</button>
            <button className={i18n.language.startsWith("ru") ? "active" : ""} onClick={() => void i18n.changeLanguage("ru")}>RU</button>
          </div>
          <div className={`engine-pill ${displayedEngineStatus}`}>
            <span className="topbar-status-dot" />
            <span>{status.stt_ready ? t("engineStatus.ready") : cloudTranscription ? t("provider.setupRequired") : t(`engineStatus.${status.engine_status}`)}</span>
            {(cloudTranscription || status.device) && <strong>{cloudTranscription ? providerName(status.stt_provider) : status.device?.toUpperCase()}</strong>}
          </div>
          <button
            className="theme-toggle"
            onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
            title={t(`theme.${theme === "dark" ? "switchToLight" : "switchToDark"}`)}
            aria-label={t(`theme.${theme === "dark" ? "switchToLight" : "switchToDark"}`)}
          >
            <Icon name={theme === "dark" ? "sun" : "moon"} size={14} />
            {t(`theme.${theme === "dark" ? "light" : "dark"}`)}
          </button>
          <button className="theme-toggle" onClick={() => setSettingsOpen(true)}>
            <Icon name="sliders" size={14} />
            {t("settings.button")}
          </button>
        </div>
      </header>

        <section className="model-strip">
        <div className="model-strip-icon"><Icon name={cloudTranscription ? "waveform" : "layers"} size={16} /></div>
        <div className="model-strip-copy">
          <span className="strip-label">{t("provider.active")}</span>
          <strong>{activeTranscriber}</strong>
        </div>
        <span className={`model-strip-status ${status.stt_ready ? "ready" : !cloudTranscription && status.model_status === "error" ? "error" : !cloudTranscription && status.model_status === "downloading" ? "working" : ""}`}>
          <span className="status-dot" />{status.stt_ready ? t("model.ready") : cloudTranscription ? t("provider.setupRequired") : modelActivity}
        </span>
        <button className="btn btn-secondary btn-sm" onClick={() => cloudTranscription ? setSettingsOpen(true) : setModelManagerOpen(true)}>
          <Icon name="sliders" size={14} />
          {cloudTranscription ? t("settings.button") : t("model.manage")}
        </button>
      </section>

      {!cloudTranscription && !status.cuda_runtime.runtime_ok && (
        <section className="card card-warning cuda-banner">
          <div className="cuda-banner-copy">
            <p className="warn-msg">{status.cuda_runtime.missing.length > 0 ? t("cuda.banner") : status.cuda_runtime.error ?? t("cuda.banner")}</p>
            {status.cuda_runtime.missing.length > 0 && (
              <p className="cuda-banner-detail">{t("cuda.missing", { dlls: status.cuda_runtime.missing.join(", ") })}</p>
            )}
            {!status.cuda_runtime.gpu_available && status.cuda_runtime.missing.length > 0 && (
              <p className="cuda-banner-detail">{t("cuda.noDriver")}</p>
            )}
            <p className="cuda-banner-detail">{t("cuda.cpuFallback")}</p>
          </div>
          <div className="cuda-banner-actions">
            {status.cuda_runtime.progress != null ? (
              <span className="cuda-banner-progress">
                <Icon name="download" size={13} />
                {t("cuda.downloading", { progress: Math.round(status.cuda_runtime.progress * 100) })}
              </span>
            ) : status.cuda_runtime.runtime_ok ? (
              <span className="cuda-banner-detail">{t("cuda.done")}</span>
            ) : status.cuda_runtime.gpu_available ? (
              <button className="btn btn-primary btn-sm" disabled={busy} onClick={() => run(() => invoke("install_cuda_runtime"))}>
                <Icon name="download" size={13} />
                {status.cuda_runtime.error ? t("cuda.retry") : t("cuda.download")}
              </button>
            ) : null}
            {status.cuda_runtime.error && status.cuda_runtime.progress == null && (
              <p className="cuda-banner-detail cuda-banner-error">{t("cuda.downloadFailed", { error: status.cuda_runtime.error })}</p>
            )}
            <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => run(() => invoke("restart_worker"))}>
              <Icon name="refresh" size={13} />
              {t("cuda.restart")}
            </button>
          </div>
        </section>
      )}

        <div className="workspace">
        <main className="main-column">
          <section className="panel recording-panel">
            <div className="section-header recording-header">
              <div className="panel-title-lockup">
                <span className="panel-icon"><Icon name="mic" size={16} /></span>
                <div>
                  <h2>{t("dictation.title")}</h2>
                  <p className="mic-line">{status.mic_name ? status.mic_name : t("session.defaultInput")}</p>
                </div>
              </div>
              <button
                className={`btn btn-record ${recording && !releasingToTalk ? "recording" : ""}`}
                disabled={!canRecord && !holdingToTalk}
                onPointerDown={(event) => {
                  if (event.currentTarget.disabled) return;
                  event.currentTarget.setPointerCapture(event.pointerId);
                  pressToTalk();
                }}
                onPointerUp={releaseToTalk}
                onPointerCancel={releaseToTalk}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    pressToTalk();
                  }
                }}
                onKeyUp={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    releaseToTalk();
                  }
                }}
                onContextMenu={(event) => event.preventDefault()}
              >
                <span className={`record-icon ${transcribing || releasingToTalk ? "spinning" : ""}`}>
                  <Icon name={transcribing || releasingToTalk ? "refresh" : "mic"} size={18} />
                </span>
                {transcribing || releasingToTalk
                  ? t("dictation.transcribing")
                  : recording
                    ? t("dictation.release")
                    : t("dictation.hold")}
              </button>
              <p className={`phase phase-${status.phase}`}>
                {recording && t("dictation.recording")}
                {!recording && !transcribing && (canRecord ? t("dictation.ready") : !cloudTranscription && status.engine_status === "loading" ? t("dictation.preparing") : cloudTranscription ? t("dictation.providerRequired") : t("dictation.modelRequired"))}
              </p>
            </div>
            <SpectrumAnalyzer
              audioLevel={status.audio_level}
              spectrum={status.audio_spectrum}
              isRecording={recording}
            />

            <div className="hotkey-row">
              <span className="hotkey-value"><Icon name="keyboard" size={15} /><span className="setting-label">{t("dictation.shortcut")}</span> {hotkeyParts(status.hotkey).map((part, index) => (
                <span key={`${part}-${index}`}>{index > 0 && " + "}<kbd>{part}</kbd></span>
              ))}</span>
              <TranscriptionProgress status={status} onCancel={cancelTranscription} />
              <button
                ref={(element) => {
                  if (capturingHotkey) element?.focus();
                }}
                className={`btn btn-ghost btn-sm hotkey-capture ${capturingHotkey ? "capturing" : ""}`}
                disabled={busy || recording || transcribing}
                onClick={() => setCapturingHotkey(true)}
                onKeyDown={captureHotkey}
              >
                {capturingHotkey ? t("dictation.pressKey") : t("dictation.edit")}
              </button>
            </div>
            {!status.hotkey_registered && <p className="warn-msg">{t("dictation.hotkeyMissing")}{status.hotkey_warning ? `: ${status.hotkey_warning}` : ""}</p>}
          </section>

          {(status.last_error || error) && (
            <section className="card card-error"><p className="error-msg">{error ?? status.last_error}</p></section>
          )}
          {status.last_warning && !status.last_error && !error && (
            <section className="card card-warning"><p className="warn-msg">{status.last_warning}</p></section>
          )}

          <section className="panel card-transcript">
            <div className="section-header">
              <div className="panel-title-lockup"><span className="panel-icon"><Icon name="clipboard" size={15} /></span><h2>{t("transcript.title")}</h2></div>
              <span className="transcript-meta">{t("transcript.clipboardOutput")}</span>
            </div>
            {status.last_text ? (
              <>
                <p className="transcript">{status.last_text}</p>
                <p className={`copied ${status.last_copied ? "ok" : "fail"}`}>
                  {status.last_copied ? t("transcript.copied") : t("transcript.copyFailed")}
                </p>
              </>
            ) : <p className="transcript empty">{t("transcript.empty")}</p>}
          </section>
        </main>

        <aside className="details-column">
          <section className="panel engine-panel">
            <div className="section-header"><div className="panel-title-lockup"><span className="panel-icon"><Icon name={cloudTranscription ? "waveform" : "cpu"} size={15} /></span><h2>{cloudTranscription ? t("provider.title") : t("engine.title")}</h2></div><span className={`mini-status ${displayedEngineStatus}`}>{status.stt_ready ? t("engineStatus.ready") : cloudTranscription ? t("provider.notConfigured") : t(`engineStatus.${status.engine_status}`)}</span></div>
            <p className="engine-detail">
              <span className={`dot dot-engine-${displayedEngineStatus}`} />
              {cloudTranscription
                ? activeTranscriber
                : status.device
                ? `${status.device === "cuda" ? "CUDA" : "CPU"}${status.compute_type ? ` В· ${status.compute_type}` : ""}`
                : status.worker_alive ? t("engine.workerOnline") : t("engine.workerOffline")}
            </p>
            {!cloudTranscription && status.engine_status === "error" && status.engine_error && <p className="error-msg">{status.engine_error}</p>}
            {!cloudTranscription && !status.worker_alive && <p className="error-msg">{t("engine.workerOffline")}</p>}
            {!cloudTranscription && currentModel?.downloaded && (
              <label className="engine-device-select">
                <span>{t("settings.deviceSelect")}</span>
                <select
                  disabled={busy || status.engine_status === "loading" || !status.worker_alive}
                  value={status.local_device || "auto"}
                  onChange={(event) => {
                    if (currentModel) {
                      run(() => invoke("load_model", { model_id: currentModel.id, device: event.target.value }));
                    }
                  }}
                >
                  <option value="auto">{t("settings.deviceAuto")}</option>
                  <option value="cuda">{t("settings.deviceCuda")}</option>
                  <option value="cpu">{t("settings.deviceCpu")}</option>
                </select>
              </label>
            )}
            <div className="actions">
              {!cloudTranscription && currentModel?.downloaded && status.engine_status !== "ready" && (
                <button className="btn btn-primary btn-sm" disabled={busy || status.engine_status === "loading" || !status.worker_alive} onClick={() => run(() => invoke("load_model", { model_id: currentModel.id }))}>
                  <Icon name="play" size={13} />
                  {status.engine_status === "loading" ? t("engine.loading") : t("engine.load")}
                </button>
              )}
              {!cloudTranscription && status.engine_status === "ready" && <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => run(() => invoke("unload_model"))}><Icon name="stop" size={13} />{t("engine.unload")}</button>}
              {!cloudTranscription && <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => run(() => invoke("restart_worker"))}><Icon name="refresh" size={13} />{t("engine.restart")}</button>}
              {cloudTranscription && <button className="btn btn-secondary btn-sm" onClick={() => setSettingsOpen(true)}><Icon name="sliders" size={13} />{t("provider.configure")}</button>}
            </div>
          </section>

          <section className="panel quick-info">
            <div className="section-header"><div className="panel-title-lockup"><span className="panel-icon"><Icon name="shield" size={15} /></span><h2>{t("session.title")}</h2></div><span className="privacy-mark">{cloudTranscription ? t("session.cloud") : t("session.localOnly")}</span></div>
            <div className="info-row"><span>{t("session.microphone")}</span><strong>{status.mic_name ?? t("session.defaultInput")}</strong></div>
            <div className="info-row"><span>{t("session.modelState")}</span><strong>{status.stt_ready ? t("model.ready") : cloudTranscription ? t("provider.setupRequired") : modelActivity}</strong></div>
            <div className="info-row"><span>{t("session.dataHandling")}</span><strong>{cloudTranscription ? t("session.sentTo", { provider: providerName(status.stt_provider) }) : t("session.staysLocal")}</strong></div>
          </section>
        </aside>
      </div>

        <footer className="privacy">{cloudTranscription ? t("privacyCloud", { provider: providerName(status.stt_provider) }) : t("privacy")}</footer>

        <ModelManager
        open={modelManagerOpen}
        onClose={() => setModelManagerOpen(false)}
        status={status}
        busy={busy}
        onRefresh={async () => {
          try {
            const s = await invoke<StatusReport>("get_status");
            setStatus(s);
          } catch (e) {
            console.error("refresh failed:", e);
          }
        }}
      />
        <SettingsModal
        open={settingsOpen}
        accent={accent}
        iconPreference={iconPreference}
        onAccentChange={setAccent}
        onIconPreferenceChange={setIconPreference}
        onClose={() => setSettingsOpen(false)}
        onSaved={async () => {
          const next = await invoke<StatusReport>("get_status");
          setStatus(next);
        }}
      />
        </div>
    </>
  );
}
