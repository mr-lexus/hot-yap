import { useEffect, useRef, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { cursorPosition, getCurrentWindow, monitorFromPoint, PhysicalPosition, primaryMonitor } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import BrandLogo from "./BrandLogo";
import Icon from "./Icons";
import type { StatusReport } from "./types";
import { applyAppearance, browserSystemTheme, resolveLogoVariant, resolvePanelTheme, savedIconPreference, type Accent, type IconPreference, type Theme } from "./appearance";
import "./overlay.css";

type OverlayMode = "idle" | "recording" | "processing" | "success" | "copy-error" | "error" | "ready";

async function positionOverlay() {
  const pointer = await cursorPosition().catch(() => null);
  const pointedMonitor = pointer
    ? await monitorFromPoint(pointer.x, pointer.y).catch(() => null)
    : null;
  const monitor = pointedMonitor ?? await primaryMonitor();
  if (!monitor) return;
  const current = getCurrentWindow();
  const size = await current.outerSize();
  const x = monitor.position.x + Math.round((monitor.size.width - size.width) / 2);
  const y = monitor.position.y + monitor.size.height - size.height - 80;
  await current.setPosition(new PhysicalPosition(x, y));
}

const EMPTY_STATUS: StatusReport = {
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
    checked: false,
    gpu_available: false,
    runtime_ok: true,
    missing: [],
    progress: null,
    error: null,
  },
  worker_install: {
    progress: null,
    error: null,
  },
  cuda_supported: true,
  provider_settings: {
    stt_provider: "local",
    text_provider: "none",
    postprocess_prompt: "",
    local_device: "auto",
    providers: {},
  },
};

export default function Overlay() {
  const { t, i18n } = useTranslation();
  const [status, setStatus] = useState(EMPTY_STATUS);
  const [mode, setMode] = useState<OverlayMode>("idle");
  const [iconPreference, setIconPreference] = useState<IconPreference>(savedIconPreference);
  const [systemTheme, setSystemTheme] = useState<Theme>(browserSystemTheme);
  const activeRef = useRef(false);
  const modeRef = useRef<OverlayMode>("idle");
  const hideTimerRef = useRef<number | null>(null);

  const changeMode = (nextMode: OverlayMode) => {
    modeRef.current = nextMode;
    setMode(nextMode);
  };

  const scheduleHide = (delay: number) => {
    if (hideTimerRef.current != null) window.clearTimeout(hideTimerRef.current);
    hideTimerRef.current = window.setTimeout(() => {
      activeRef.current = false;
      changeMode("idle");
      void getCurrentWindow().hide();
    }, delay);
  };

  useEffect(() => {
    document.documentElement.classList.add("overlay-page");
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];
    const systemThemeQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const syncBrowserTheme = () => setSystemTheme(browserSystemTheme());
    systemThemeQuery.addEventListener("change", syncBrowserTheme);

    void invoke<StatusReport>("get_status").then((current) => {
      if (!cancelled) setStatus(current);
    }).catch(() => {});
    void positionOverlay().catch(() => {});

    void (async () => {
      unlisteners.push(await listen("hotyap:overlay-open", () => {
        void positionOverlay().catch(() => {});
        window.setTimeout(() => {
          void getCurrentWindow().setIgnoreCursorEvents(true).catch(() => {});
        }, 50);
        activeRef.current = true;
        if (hideTimerRef.current != null) window.clearTimeout(hideTimerRef.current);
        changeMode("recording");
      }));
      unlisteners.push(await listen<{ language: "ru" | "en" }>("hotyap:language", (event) => {
        void i18n.changeLanguage(event.payload.language);
      }));
      unlisteners.push(await listen<{ theme: Theme; accent: Accent }>("hotyap:appearance", (event) => {
        applyAppearance(event.payload.theme, event.payload.accent);
      }));
      unlisteners.push(await listen<{ preference: IconPreference }>("hotyap:icon-preference", (event) => {
        setIconPreference(event.payload.preference);
      }));
      unlisteners.push(await getCurrentWindow().onThemeChanged((event) => {
        setSystemTheme(event.payload);
      }));
      void getCurrentWindow().theme().then((value) => {
        if (!cancelled && value) setSystemTheme(value);
      }).catch(() => {});
      unlisteners.push(await listen<{ message?: string }>("hotyap:overlay-error", () => {
        if (!activeRef.current) return;
        changeMode("error");
        scheduleHide(2600);
      }));
      unlisteners.push(await listen("hotyap:model-ready", () => {
        void positionOverlay().catch(() => {});
        activeRef.current = true;
        if (hideTimerRef.current != null) window.clearTimeout(hideTimerRef.current);
        changeMode("ready");
        scheduleHide(2200);
      }));
      unlisteners.push(await listen<StatusReport>("vox:status", (event) => {
        const next = event.payload;
        setStatus(next);
        if (!activeRef.current) return;

        if (next.phase === "recording") {
          changeMode("recording");
          return;
        }
        if (next.phase === "transcribing") {
          changeMode("processing");
          return;
        }
        if (modeRef.current === "recording" || modeRef.current === "processing") {
          if (next.last_error === "Transcription cancelled") {
            activeRef.current = false;
            changeMode("idle");
            void getCurrentWindow().hide();
          } else if (next.last_error) {
            changeMode("error");
            scheduleHide(2600);
          } else if (!next.last_copied) {
            changeMode("copy-error");
            scheduleHide(2600);
          } else {
            changeMode("success");
            scheduleHide(1800);
          }
        }
      }));
      unlisteners.push(await listen<{ elapsed: number; fraction?: number }>("vox:transcribe-progress", (event) => {
        setStatus((previous) => ({
          ...previous,
          transcribe_elapsed: event.payload.elapsed,
          transcribe_progress: event.payload.fraction ?? previous.transcribe_progress,
        }));
      }));
      unlisteners.push(await listen<{ level: number; spectrum: number[] }>("vox:audio-meter", (event) => {
        setStatus((previous) => ({
          ...previous,
          audio_level: event.payload.level,
          audio_spectrum: event.payload.spectrum,
        }));
      }));
    })();

    return () => {
      cancelled = true;
      document.documentElement.classList.remove("overlay-page");
      systemThemeQuery.removeEventListener("change", syncBrowserTheme);
      if (hideTimerRef.current != null) window.clearTimeout(hideTimerRef.current);
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  const cancelTranscription = () => {
    invoke("cancel_transcription").catch(() => {});
  };

  useEffect(() => {
    if (mode !== "processing") return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        cancelTranscription();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [mode]);

  useEffect(() => {
    if (mode === "processing") {
      void getCurrentWindow().setIgnoreCursorEvents(false).catch(() => {});
    } else if (activeRef.current) {
      void getCurrentWindow().setIgnoreCursorEvents(true).catch(() => {});
    }
  }, [mode]);

  const panelTheme = resolvePanelTheme(iconPreference, systemTheme);
  const logoVariant = resolveLogoVariant(iconPreference, systemTheme);
  const bars = status.audio_spectrum.length >= 20 ? status.audio_spectrum.slice(0, 20) : Array(20).fill(0);
  const voiceIntensity = mode === "recording"
    ? Math.min(1, Math.max(0, (status.audio_level - 0.01) * 10))
    : 0;
  const shake = 0.8 + voiceIntensity * 5.2;
  const tilt = 1.5 + voiceIntensity * 7.5;
  const logoMotionStyle = {
    "--logo-shake": `${shake}px`,
    "--logo-shake-negative": `${-shake}px`,
    "--logo-shake-small": `${shake * 0.35}px`,
    "--logo-shake-small-negative": `${shake * -0.55}px`,
    "--logo-shake-medium": `${shake * 0.7}px`,
    "--logo-shake-medium-negative": `${shake * -0.8}px`,
    "--logo-tilt": `${tilt}deg`,
    "--logo-tilt-negative": `${-tilt}deg`,
    "--logo-tilt-half": `${tilt * 0.5}deg`,
    "--logo-tilt-half-negative": `${tilt * -0.45}deg`,
    "--logo-punch": `${1 + voiceIntensity * 0.07}`,
    "--logo-shake-speed": `${Math.round(165 - voiceIntensity * 55)}ms`,
    "--logo-rage-speed": `${Math.round(260 - voiceIntensity * 80)}ms`,
  } as CSSProperties;
  const copy = mode === "recording"
    ? { title: t("overlay.recording"), hint: t("overlay.recordingHint") }
    : mode === "processing"
      ? {
          title: t("overlay.processing"),
          hint: t(status.stt_provider === "local" ? "overlay.processingUnknown" : "overlay.processingCloudUnknown"),
        }
      : mode === "success"
        ? { title: t("overlay.copied"), hint: t("overlay.copiedHint") }
        : mode === "copy-error"
          ? { title: t("overlay.copyError"), hint: t("overlay.copyErrorHint") }
          : mode === "ready"
            ? { title: t("overlay.ready"), hint: t("overlay.readyHint") }
            : { title: t("overlay.error"), hint: t("overlay.errorHint") };

  return (
    <main className={`ptt-overlay overlay-theme-${panelTheme} mode-${mode}`}>
      <div
        className={`overlay-logo ${voiceIntensity > 0.08 ? "is-talking" : ""}`}
        style={logoMotionStyle}
      >
        <BrandLogo size={52} variant={logoVariant} />
      </div>
      <div className="overlay-copy" role="status" aria-live="polite">
        <strong>{copy.title}</strong>
        <span>{copy.hint}</span>
      </div>
      <div className="overlay-visual">
        {mode === "recording" && (
          <div className="overlay-spectrum" aria-hidden="true">
            {bars.map((value, index) => (
              <span
                key={index}
                style={{
                  height: `${Math.max(12, Math.min(100, Math.pow(Math.max(value, 0), 0.65) * 100))}%`,
                  animationDuration: `${1100 + index * 55}ms`,
                  animationDelay: `${index * -65}ms`,
                }}
              />
            ))}
          </div>
        )}
        {mode === "processing" && (
          <div className="overlay-progress" role="status" aria-label={t("overlay.processing")}>
            <span className="overlay-spinner" aria-hidden="true" />
            <Icon name="waveform" size={16} />
            <button
              className="overlay-cancel-btn"
              onClick={cancelTranscription}
              title={t("progress.cancel")}
              aria-label={t("progress.cancel")}
            >
              <Icon name="close" size={14} />
            </button>
          </div>
        )}
        {(mode === "success" || mode === "ready") && <span className="overlay-result success"><Icon name="check" size={22} /></span>}
        {(mode === "error" || mode === "copy-error" || mode === "idle") && <span className="overlay-result error"><Icon name="close" size={21} /></span>}
      </div>
    </main>
  );
}
