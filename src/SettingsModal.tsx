import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import type { Accent, IconPreference } from "./appearance";
import { ACCENTS, ICON_PREFERENCES } from "./appearance";
import Icon from "./Icons";
import type { ProviderConfig, ProviderSettings } from "./types";

interface SettingsModalProps {
  open: boolean;
  accent: Accent;
  iconPreference: IconPreference;
  onAccentChange: (accent: Accent) => void;
  onIconPreferenceChange: (preference: IconPreference) => void;
  onClose: () => void;
  onSaved: () => Promise<void>;
}

const PROVIDER_NAMES: Record<string, string> = {
  local: "Local Whisper",
  none: "None",
  openai: "OpenAI",
  deepgram: "Deepgram",
  groq: "Groq",
  elevenlabs: "ElevenLabs",
  assemblyai: "AssemblyAI",
  gemini: "Google Gemini",
  openrouter: "OpenRouter",
  anthropic: "Anthropic",
  xai: "xAI",
  bedrock: "Amazon Bedrock",
  ollama: "Ollama",
  lmstudio: "LM Studio",
};

const STT_PROVIDERS = ["local", "openai", "deepgram", "groq", "elevenlabs", "assemblyai", "gemini"];
const TEXT_PROVIDERS = ["none", "openai", "groq", "openrouter", "anthropic", "gemini", "xai", "bedrock", "ollama", "lmstudio"];
const OPTIONAL_KEY = new Set(["ollama", "lmstudio"]);

export const providerName = (id: string) => PROVIDER_NAMES[id] ?? id;

export default function SettingsModal({ open, accent, iconPreference, onAccentChange, onIconPreferenceChange, onClose, onSaved }: SettingsModalProps) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<ProviderSettings | null>(null);
  const [secrets, setSecrets] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const savingRef = useRef(saving);
  const onCloseRef = useRef(onClose);
  savingRef.current = saving;
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!open) return;
    let current = true;
    setSettings(null);
    setError(null);
    setSaved(false);
    setSecrets({});
    void invoke<ProviderSettings>("get_provider_settings")
      .then((value) => { if (current) setSettings(value); })
      .catch((value) => { if (current) setError(String(value)); });
    return () => { current = false; };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    previousFocusRef.current = document.activeElement as HTMLElement | null;
    window.requestAnimationFrame(() => closeButtonRef.current?.focus());
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape" || savingRef.current) return;
      event.preventDefault();
      onCloseRef.current();
      previousFocusRef.current?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open]);

  if (!open) return null;

  const updateProvider = (id: string, patch: Partial<ProviderConfig>) => {
    setSettings((current) => current ? {
      ...current,
      providers: {
        ...current.providers,
        [id]: { ...current.providers[id], ...patch },
      },
    } : current);
    setSaved(false);
  };

  const save = async () => {
    if (!settings) return;
    setSaving(true);
    setError(null);
    try {
      const next = await invoke<ProviderSettings>("save_provider_settings", { settings, secrets });
      setSettings(next);
      setSecrets({});
      setSaved(true);
      await onSaved();
    } catch (value) {
      setError(String(value));
    } finally {
      setSaving(false);
    }
  };

  const deleteKey = async (provider: string) => {
    if (!window.confirm(t("settings.deleteKeyConfirm", { provider: providerName(provider) }))) return;
    setSaving(true);
    setError(null);
    try {
      const next = await invoke<ProviderSettings>("delete_provider_secret", { provider });
      setSettings(next);
      setSecrets((current) => ({ ...current, [provider]: "" }));
      await onSaved();
    } catch (value) {
      setError(String(value));
    } finally {
      setSaving(false);
    }
  };

  const displayProviderName = (id: string) => {
    if (id === "local") return t("settings.localProvider");
    if (id === "none") return t("settings.noProvider");
    return providerName(id);
  };

  const moveAccent = (event: KeyboardEvent<HTMLButtonElement>, value: Accent) => {
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
    event.preventDefault();
    const direction = event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1;
    const next = ACCENTS[(ACCENTS.indexOf(value) + direction + ACCENTS.length) % ACCENTS.length];
    onAccentChange(next);
    document.querySelector<HTMLButtonElement>(`[data-accent-option="${next}"]`)?.focus();
  };

  const providerFields = (provider: string, capability: "stt" | "text") => {
    const config = settings?.providers[provider];
    if (!config) return null;
    const modelKey = capability === "stt" ? "stt_model" : "text_model";
    return (
      <div className="provider-config">
        <div className="provider-config-heading">
          <div>
            <strong>{displayProviderName(provider)}</strong>
            <span>{config.api_key_set ? t("settings.keySaved") : OPTIONAL_KEY.has(provider) ? t("settings.keyOptional") : t("settings.keyMissing")}</span>
          </div>
          {config.api_key_set && (
            <button className="btn btn-ghost btn-sm" disabled={saving} onClick={() => void deleteKey(provider)}>{t("settings.deleteKey")}</button>
          )}
        </div>
        <div className="settings-field-grid">
          <label className="settings-field">
            <span>{t("settings.model")}</span>
            <input disabled={saving} value={config[modelKey]} onChange={(event) => updateProvider(provider, { [modelKey]: event.target.value })} />
          </label>
          <label className="settings-field">
            <span>{t("settings.apiKey")}{OPTIONAL_KEY.has(provider) ? ` · ${t("settings.optional")}` : ""}</span>
            <input
              autoComplete="off"
              disabled={saving}
              placeholder={config.api_key_set ? t("settings.keyUnchanged") : t("settings.keyPlaceholder")}
              type="password"
              value={secrets[provider] ?? ""}
              onChange={(event) => {
                setSecrets((current) => ({ ...current, [provider]: event.target.value }));
                setSaved(false);
              }}
            />
          </label>
          <label className="settings-field settings-field-wide">
            <span>{t("settings.endpoint")}</span>
            <input disabled={saving} value={config.endpoint} onChange={(event) => updateProvider(provider, { endpoint: event.target.value })} />
          </label>
        </div>
      </div>
    );
  };

  return (
    <div className="modal-backdrop settings-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && !saving && onClose()}>
      <section className="settings-modal" role="dialog" aria-modal="true" aria-labelledby="settings-title">
        <header className="modal-header">
          <div>
            <h2 id="settings-title">{t("settings.title")}</h2>
            <p className="modal-subtitle">{t("settings.subtitle")}</p>
          </div>
          <button ref={closeButtonRef} className="modal-icon-button" aria-label={t("settings.close")} disabled={saving} onClick={onClose}><Icon name="close" size={17} /></button>
        </header>

        <div className={`settings-scroll ${saving ? "saving" : ""}`} aria-busy={saving}>
          <section className="settings-section">
            <div className="settings-section-heading">
              <span className="panel-icon"><Icon name="sun" size={15} /></span>
              <div><h3>{t("settings.appearance")}</h3><p>{t("settings.appearanceHint")}</p></div>
            </div>
            <div className="accent-options" role="radiogroup" aria-label={t("settings.accent")}>
              {ACCENTS.map((value) => (
                <button
                  key={value}
                  className={`accent-option ${accent === value ? "active" : ""}`}
                  data-accent-option={value}
                  onClick={() => onAccentChange(value)}
                  onKeyDown={(event) => moveAccent(event, value)}
                  role="radio"
                  aria-checked={accent === value}
                  tabIndex={accent === value ? 0 : -1}
                  disabled={saving}
                >
                  <span className={`accent-swatch accent-${value}`} />
                  {t(`settings.accents.${value}`)}
                </button>
              ))}
            </div>
            <div className="icon-preference-heading">
              <strong>{t("settings.appIcon")}</strong>
              <span>{t("settings.appIconHint")}</span>
            </div>
            <div className="icon-mode-options">
              {ICON_PREFERENCES.map((value) => (
                <label key={value} className={`icon-mode-option ${iconPreference === value ? "active" : ""}`}>
                  <input
                    type="radio"
                    name="icon-preference"
                    value={value}
                    checked={iconPreference === value}
                    disabled={saving}
                    onChange={() => onIconPreferenceChange(value)}
                  />
                  <span className={`icon-mode-preview icon-mode-${value}`} aria-hidden="true">
                    {value === "system" ? (
                      <><img className="preview-dark" src="/dark.png" alt="" /><img className="preview-light" src="/light.png" alt="" /></>
                    ) : <img src={value === "dark-panel" ? "/light.png" : "/dark.png"} alt="" />}
                  </span>
                  <span className="icon-mode-copy">
                    <strong>{t(`settings.iconModes.${value}.title`)}</strong>
                    <small>{t(`settings.iconModes.${value}.hint`)}</small>
                  </span>
                </label>
              ))}
            </div>
          </section>

          <section className="settings-section">
            <div className="settings-section-heading">
              <span className="panel-icon"><Icon name="mic" size={15} /></span>
              <div><h3>{t("settings.transcription")}</h3><p>{t("settings.transcriptionHint")}</p></div>
            </div>
            {settings ? (
              <>
                <label className="settings-field settings-provider-select">
                  <span>{t("settings.provider")}</span>
                  <select disabled={saving} value={settings.stt_provider} onChange={(event) => { setSettings({ ...settings, stt_provider: event.target.value }); setSaved(false); }}>
                    {STT_PROVIDERS.map((provider) => <option key={provider} value={provider}>{displayProviderName(provider)}</option>)}
                  </select>
                </label>
                {settings.stt_provider === "local" ? (
                  <>
                    <p className="settings-note"><Icon name="lock" size={14} />{t("settings.localHint")}</p>
                    <label className="settings-field settings-provider-select" style={{ marginTop: "12px" }}>
                      <span>{t("settings.deviceSelect")}</span>
                      <select
                        disabled={saving}
                        value={settings.local_device || "auto"}
                        onChange={(event) => {
                          setSettings({ ...settings, local_device: event.target.value });
                          setSaved(false);
                        }}
                      >
                        <option value="auto">{t("settings.deviceAuto")}</option>
                        <option value="cuda">{t("settings.deviceCuda")}</option>
                        <option value="cpu">{t("settings.deviceCpu")}</option>
                      </select>
                    </label>
                  </>
                ) : (
                  providerFields(settings.stt_provider, "stt")
                )}
              </>
            ) : <p className="settings-note">{t("settings.loading")}</p>}
          </section>

          <section className="settings-section">
            <div className="settings-section-heading">
              <span className="panel-icon"><Icon name="waveform" size={15} /></span>
              <div><h3>{t("settings.processing")}</h3><p>{t("settings.processingHint")}</p></div>
            </div>
            {settings && (
              <>
                <label className="settings-field settings-provider-select">
                  <span>{t("settings.provider")}</span>
                  <select disabled={saving} value={settings.text_provider} onChange={(event) => { setSettings({ ...settings, text_provider: event.target.value }); setSaved(false); }}>
                    {TEXT_PROVIDERS.map((provider) => <option key={provider} value={provider}>{displayProviderName(provider)}</option>)}
                  </select>
                </label>
                {settings.text_provider !== "none" && (
                  <>
                    {providerFields(settings.text_provider, "text")}
                    <label className="settings-field settings-prompt">
                      <span>{t("settings.instruction")}</span>
                      <textarea disabled={saving} value={settings.postprocess_prompt} onChange={(event) => { setSettings({ ...settings, postprocess_prompt: event.target.value }); setSaved(false); }} />
                    </label>
                  </>
                )}
              </>
            )}
          </section>

          <p className="credential-note"><Icon name="shield" size={15} />{t("settings.credentialNote")}</p>
          {error && <p className="settings-error">{error}</p>}
        </div>

        <footer className="settings-footer">
          {saved && <span className="settings-saved"><Icon name="check" size={14} />{t("settings.saved")}</span>}
          <button className="btn btn-ghost" disabled={saving} onClick={onClose}>{t("settings.cancel")}</button>
          <button className="btn btn-primary" disabled={!settings || saving} onClick={() => void save()}>{saving ? t("settings.saving") : t("settings.save")}</button>
        </footer>
      </section>
    </div>
  );
}
