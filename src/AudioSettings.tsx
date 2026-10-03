import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import Icon from "./Icons";
import type { ProviderSettings } from "./types";

interface Device {
  id: string;
  name: string;
  is_default: boolean;
}
interface TestReport {
  active: boolean;
  level: number;
  remaining: number;
  mic_name: string | null;
  error: string | null;
}
interface Props {
  settings: ProviderSettings;
  disabled: boolean;
  onChange: (settings: ProviderSettings) => void;
}

export default function AudioSettings({ settings, disabled, onChange }: Props) {
  const { t } = useTranslation();
  const [devices, setDevices] = useState<Device[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [test, setTest] = useState<TestReport | null>(null);
  const [starting, setStarting] = useState(false);
  const [heard, setHeard] = useState(false);
  const session = useRef<string | null>(null);
  const mounted = useRef(true);
  const opening = useRef(false);
  const refreshGeneration = useRef(0);

  const refresh = async () => {
    const generation = ++refreshGeneration.current;
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<Device[]>("list_microphones");
      if (mounted.current && generation === refreshGeneration.current)
        setDevices(next);
    } catch (e) {
      if (mounted.current && generation === refreshGeneration.current)
        setError(String(e));
    } finally {
      if (mounted.current && generation === refreshGeneration.current)
        setLoading(false);
    }
  };
  const stop = () => {
    const id = session.current;
    session.current = null;
    if (id)
      void invoke("stop_microphone_test", { id }).catch((e) => {
        if (mounted.current) setError(String(e));
      });
    setStarting(opening.current);
    setTest((current) =>
      current ? { ...current, active: false, level: 0 } : null,
    );
  };
  const start = async () => {
    const id = crypto.randomUUID();
    session.current = id;
    opening.current = true;
    setStarting(true);
    setError(null);
    setTest(null);
    setHeard(false);
    try {
      await invoke("start_microphone_test", {
        id,
        device: settings.input_device,
      });
      if (!mounted.current || session.current !== id) {
        await invoke("stop_microphone_test", { id });
        return;
      }
      setTest({
        active: true,
        level: 0,
        remaining: 15,
        mic_name: null,
        error: null,
      });
    } catch (e) {
      if (mounted.current && session.current === id) {
        setError(String(e));
        session.current = null;
      }
    } finally {
      opening.current = false;
      if (mounted.current) setStarting(false);
    }
  };

  useEffect(() => {
    mounted.current = true;
    void refresh();
    // Refresh when returning from OS audio settings; manual refresh also works
    // on webviews which don't expose mediaDevices/devicechange.
    const reload = () => {
      void refresh();
    };
    window.addEventListener("focus", reload);
    navigator.mediaDevices?.addEventListener("devicechange", reload);
    let pending = false;
    const timer = window.setInterval(async () => {
      const id = session.current;
      if (!id || pending || opening.current) return;
      pending = true;
      try {
        const next = await invoke<TestReport>("microphone_test_status", { id });
        if (!mounted.current || session.current !== id) return;
        setTest(next);
        if (next.level > 0.008) setHeard(true);
        if (next.error) setError(next.error);
        if (!next.active) session.current = null;
      } catch (e) {
        if (mounted.current && session.current === id) {
          setError(String(e));
          stop();
        }
      } finally {
        pending = false;
      }
    }, 100);
    return () => {
      mounted.current = false;
      window.clearInterval(timer);
      window.removeEventListener("focus", reload);
      navigator.mediaDevices?.removeEventListener("devicechange", reload);
      const id = session.current;
      session.current = null;
      if (id) void invoke("stop_microphone_test", { id }).catch(() => {});
    };
    // Lifecycle uses refs to keep asynchronous results scoped to this mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const missing =
    settings.input_device !== null &&
    !devices.some((d) => d.id === settings.input_device);
  const defaultDevice = devices.find((d) => d.is_default);
  const meter = Math.min(100, Math.max(0, (test?.level ?? 0) * 500));
  return (
    <>
      <section className="settings-section">
        <div className="settings-section-heading">
          <span className="panel-icon">
            <Icon name="waveform" size={15} />
          </span>
          <div>
            <h3>{t("audioSettings.title")}</h3>
            <p>{t("audioSettings.subtitle")}</p>
          </div>
        </div>
        <div className="dictation-setting microphone-device">
          <label htmlFor="input-device">
            <strong>{t("audioSettings.microphone")}</strong>
          </label>
          <div className="microphone-device-row">
            <select
              id="input-device"
              value={settings.input_device ?? ""}
              disabled={disabled || starting || loading}
              onChange={(e) => {
                stop();
                setTest(null);
                onChange({ ...settings, input_device: e.target.value || null });
              }}
            >
              <option value="">{t("audioSettings.default")}</option>
              {missing && (
                <option value={settings.input_device!}>
                  {t("audioSettings.disconnected")}
                </option>
              )}
              {devices.map((device) => (
                <option key={device.id} value={device.id}>
                  {device.name}
                  {device.is_default ? ` · ${t("audioSettings.system")}` : ""}
                </option>
              ))}
            </select>
            <button
              className="btn btn-ghost btn-sm"
              disabled={loading || disabled}
              onClick={() => void refresh()}
            >
              {t("audioSettings.refresh")}
            </button>
          </div>
          <p>
            {settings.input_device
              ? t("audioSettings.fixedHint")
              : t("audioSettings.defaultHint", {
                  name: defaultDevice?.name ?? t("audioSettings.noDevice"),
                })}
          </p>
          {!loading && missing && (
            <p className="microphone-warning" role="status">
              {t("audioSettings.missingHint")}
            </p>
          )}
        </div>
        <div className={`microphone-test ${test?.active ? "is-testing" : ""}`}>
          <div className="microphone-test-heading">
            <div>
              <strong>{t("audioSettings.test")}</strong>
              <p>{t("audioSettings.testHint")}</p>
            </div>
            <button
              className="btn btn-ghost btn-sm"
              disabled={
                disabled ||
                (starting && !session.current) ||
                (!session.current &&
                  (loading || missing || devices.length === 0))
              }
              onClick={() => (session.current ? stop() : void start())}
            >
              {starting || test?.active
                ? t("audioSettings.stop")
                : t("audioSettings.start")}
            </button>
          </div>
          <div
            className="microphone-level"
            role="meter"
            aria-label={t("audioSettings.level")}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(meter)}
          >
            <span style={{ width: `${meter}%` }} />
          </div>
          <div className="microphone-test-status" role="status">
            <span>
              {starting
                ? t("audioSettings.opening")
                : test?.active
                  ? t("audioSettings.speak")
                  : test
                    ? t(
                        heard
                          ? "audioSettings.detected"
                          : "audioSettings.silent",
                      )
                    : t("audioSettings.ready")}
            </span>
            {test?.active && (
              <span>
                {t("audioSettings.seconds", { count: test.remaining })}
              </span>
            )}
          </div>
          {test?.mic_name && (
            <p className="microphone-test-device">{test.mic_name}</p>
          )}
        </div>
        {error && (
          <p className="microphone-warning" role="alert">
            {error}
          </p>
        )}
        <div className="dictation-setting">
          <label>
            <strong>{t("audioSettings.output")}</strong>
            <select
              aria-label={t("audioSettings.output")}
              value={settings.system_audio}
              disabled={disabled}
              onChange={(e) =>
                onChange({
                  ...settings,
                  system_audio: e.target
                    .value as ProviderSettings["system_audio"],
                })
              }
            >
              <option value="nothing">{t("audioSettings.nothing")}</option>
              <option value="duck">{t("audioSettings.duck")}</option>
              <option value="mute">{t("audioSettings.mute")}</option>
            </select>
          </label>
          <p>{t("audioSettings.outputHint")}</p>
          {settings.system_audio !== "nothing" && (
            <p>{t("audioSettings.outputCompatibility")}</p>
          )}
        </div>
      </section>
      <section className="settings-section">
        <div className="settings-section-heading">
          <div>
            <h3>{t("audioSettings.application")}</h3>
            <p>{t("audioSettings.applicationHint")}</p>
          </div>
        </div>
        <div className="dictation-setting">
          <label>
            <strong>{t("audioSettings.startup")}</strong>
            <input
              type="checkbox"
              checked={settings.launch_at_startup}
              disabled={disabled}
              onChange={(e) =>
                onChange({ ...settings, launch_at_startup: e.target.checked })
              }
            />
          </label>
          <p>{t("audioSettings.startupHint")}</p>
        </div>
        <div className="dictation-setting">
          <label>
            <strong>{t("audioSettings.tray")}</strong>
            <input
              type="checkbox"
              checked={settings.close_to_tray}
              disabled={disabled}
              onChange={(e) =>
                onChange({ ...settings, close_to_tray: e.target.checked })
              }
            />
          </label>
          <p>
            {t(
              settings.close_to_tray
                ? "audioSettings.trayHint"
                : "audioSettings.quitHint",
            )}
          </p>
        </div>
      </section>
    </>
  );
}
