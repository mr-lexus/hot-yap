import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import BrandLogo from "./BrandLogo";
import Icon from "./Icons";
import type { Theme } from "./appearance";
import type { StatusReport } from "./types";
import { providerName } from "./SettingsModal";

interface TitleBarProps {
  theme: Theme;
  status: StatusReport;
  onModelSwitch: (target: string) => void;
}

interface ModelOption {
  id: string;
  label: string;
  group: "local" | "cloud";
  active: boolean;
}

export default function TitleBar({ theme, status, onModelSwitch }: TitleBarProps) {
  const { t } = useTranslation();
  const [maximized, setMaximized] = useState(false);
  const [focused, setFocused] = useState(true);
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  const [menuPos, setMenuPos] = useState<{ top: number; left: number }>({ top: 38, left: 0 });
  const menuRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

  const isCloud = status.stt_provider !== "local";

  const localModels: ModelOption[] = status.models
    .filter((m) => m.downloaded)
    .map((m) => ({
      id: m.id,
      label: m.name,
      group: "local" as const,
      active: !isCloud && status.current_model_id === m.id,
    }));

  const cloudProviders: ModelOption[] = (() => {
    const sttProviders = ["openai", "deepgram", "groq", "elevenlabs", "assemblyai", "gemini"];
    return sttProviders
      .filter((pid) => {
        if (isCloud && status.stt_provider === pid) return true;
        const config = status.provider_settings.providers[pid];
        return config?.api_key_set;
      })
      .map((pid) => ({
        id: pid,
        label: providerName(pid),
        group: "cloud" as const,
        active: isCloud && status.stt_provider === pid,
      }));
  })();

  const currentLabel = isCloud
    ? providerName(status.stt_provider)
    : status.models.find((m) => m.id === status.current_model_id)?.name ?? t("model.none");

  const hasOptions = localModels.length > 0 || cloudProviders.length > 0;

  useEffect(() => {
    let unlisten: Array<() => void> = [];
    let disposed = false;

    (async () => {
      try {
        const window = getCurrentWindow();
        if (disposed) return;
        setMaximized(await window.isMaximized());
        setFocused(await window.isFocused());

        const un1 = await window.onResized(() => {
          void window.isMaximized().then(setMaximized).catch(() => {});
        });
        const un2 = await window.onFocusChanged(({ payload }) => setFocused(payload));
        unlisten = [un1, un2];
      } catch (e) {
        console.error("titlebar window state failed:", e);
      }
    })();

    return () => {
      disposed = true;
      unlisten.forEach((un) => un());
    };
  }, []);

  useEffect(() => {
    if (!modelMenuOpen) return;
    const handleDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setModelMenuOpen(false);
      }
    };
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setModelMenuOpen(false);
    };
    document.addEventListener("mousedown", handleDown);
    document.addEventListener("keydown", handleKey);
    return () => {
      document.removeEventListener("mousedown", handleDown);
      document.removeEventListener("keydown", handleKey);
    };
  }, [modelMenuOpen]);

  const toggleModelMenu = () => {
    if (!hasOptions) return;
    if (!modelMenuOpen && buttonRef.current) {
      const rect = buttonRef.current.getBoundingClientRect();
      setMenuPos({ top: rect.bottom + 2, left: rect.left });
    }
    setModelMenuOpen((prev) => !prev);
  };

  const selectOption = (option: ModelOption) => {
    setModelMenuOpen(false);
    if (option.active) return;
    const target = option.group === "cloud" ? `cloud:${option.id}` : `local:${option.id}`;
    onModelSwitch(target);
  };

  const minimize = () => void getCurrentWindow().minimize();
  const toggleMaximize = () => {
    const window = getCurrentWindow();
    void (maximized ? window.unmaximize() : window.maximize());
  };
  const close = () => void getCurrentWindow().close();

  return (
    <header className={`titlebar ${focused ? "" : "unfocused"}`} data-tauri-drag-region>
      <div className="titlebar-brand" data-tauri-drag-region>
        <span className="titlebar-logo" data-tauri-drag-region>
          <BrandLogo size={20} variant={theme === "dark" ? "light" : "dark"} />
        </span>
        <span className="titlebar-name" data-tauri-drag-region>{t("brand.name")}</span>
        <span className="titlebar-tagline" data-tauri-drag-region>{t("brand.kicker")}</span>
      </div>

      {hasOptions && (
        <>
          <div className="titlebar-spacer" />
          <button
            ref={buttonRef}
            className={`titlebar-model-selector ${modelMenuOpen ? "open" : ""}`}
            onClick={toggleModelMenu}
            title={t("modelSwitcher.tooltip")}
          >
            <Icon name={isCloud ? "waveform" : "layers"} size={12} />
            <span className="titlebar-model-name">{currentLabel}</span>
            <svg className="titlebar-model-chevron" width="7" height="7" viewBox="0 0 4 5">
              <path d="M2 5L0 0L4 0Z" fill="currentColor" />
            </svg>
          </button>
        </>
      )}

      {modelMenuOpen && (
        <div
          ref={menuRef}
          className="titlebar-model-menu"
          style={{ position: "fixed", top: menuPos.top, left: menuPos.left }}
        >
          {localModels.length > 0 && (
            <>
              <div className="titlebar-model-group-label">{t("modelSwitcher.local")}</div>
              {localModels.map((opt) => (
                <button
                  key={opt.id}
                  className={`titlebar-model-option ${opt.active ? "active" : ""}`}
                  onClick={() => selectOption(opt)}
                >
                  <Icon name="layers" size={12} />
                  <span>{opt.label}</span>
                  {opt.active && <Icon name="check" size={12} />}
                </button>
              ))}
            </>
          )}
          {cloudProviders.length > 0 && (
            <>
              {localModels.length > 0 && <div className="titlebar-model-separator" />}
              <div className="titlebar-model-group-label">{t("modelSwitcher.cloud")}</div>
              {cloudProviders.map((opt) => (
                <button
                  key={opt.id}
                  className={`titlebar-model-option ${opt.active ? "active" : ""}`}
                  onClick={() => selectOption(opt)}
                >
                  <Icon name="waveform" size={12} />
                  <span>{opt.label}</span>
                  {opt.active && <Icon name="check" size={12} />}
                </button>
              ))}
            </>
          )}
        </div>
      )}

      <div className="titlebar-controls">
        <button
          className="titlebar-control"
          onClick={minimize}
          title={t("window.minimize")}
          aria-label={t("window.minimize")}
        >
          <Icon name="minimize" size={15} />
        </button>
        <button
          className="titlebar-control"
          onClick={toggleMaximize}
          title={t(maximized ? "window.restore" : "window.maximize")}
          aria-label={t(maximized ? "window.restore" : "window.maximize")}
        >
          <Icon name={maximized ? "restore" : "maximize"} size={13} />
        </button>
        <button
          className="titlebar-control titlebar-control-close"
          onClick={close}
          title={t("window.close")}
          aria-label={t("window.close")}
        >
          <Icon name="close" size={15} />
        </button>
      </div>
    </header>
  );
}