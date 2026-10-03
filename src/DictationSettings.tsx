import { useTranslation } from "react-i18next";
import Icon from "./Icons";
import type { PasteSupport, ProviderSettings } from "./types";

interface Props {
  settings: ProviderSettings;
  pasteSupport: PasteSupport | null;
  disabled: boolean;
  onChange: (settings: ProviderSettings) => void;
}

export default function DictationSettings({
  settings,
  pasteSupport,
  disabled,
  onChange,
}: Props) {
  const { t } = useTranslation();
  return (
    <section className="settings-section">
      <div className="settings-section-heading">
        <span className="panel-icon">
          <Icon name="waveform" size={15} />
        </span>
        <div>
          <h3>{t("dictationFlow.title")}</h3>
          <p>{t("dictationFlow.subtitle")}</p>
        </div>
      </div>
      <div className="dictation-setting">
        <label>
          <strong>{t("dictationFlow.live")}</strong>
          <input
            type="checkbox"
            checked={settings.live_transcription}
            disabled={disabled}
            onChange={(e) => {
              onChange({ ...settings, live_transcription: e.target.checked });
            }}
          />
        </label>
        <p>{t("dictationFlow.liveHint")}</p>
        {settings.stt_provider !== "local" && (
          <p>{t("dictationFlow.localOnly")}</p>
        )}
      </div>
      {settings.live_transcription && (
        <div className="dictation-setting">
          <label>
            <strong>{t("dictationFlow.showPreview")}</strong>
            <input
              type="checkbox"
              checked={settings.dictation_preview}
              disabled={disabled}
              onChange={(e) =>
                onChange({ ...settings, dictation_preview: e.target.checked })
              }
            />
          </label>
          <p>{t("dictationFlow.showPreviewHint")}</p>
        </div>
      )}
      {settings.live_transcription && (
        <div className="dictation-setting">
          <label>
            <span>{t("dictationFlow.finalization")}</span>
            <select
              disabled={disabled}
              value={settings.live_final_pass ? "accurate" : "fast"}
              onChange={(e) => {
                onChange({
                  ...settings,
                  live_final_pass: e.target.value === "accurate",
                });
              }}
            >
              <option value="accurate">{t("dictationFlow.accurate")}</option>
              <option value="fast">{t("dictationFlow.fast")}</option>
            </select>
          </label>
          <p>
            {t(
              settings.live_final_pass
                ? "dictationFlow.accurateHint"
                : "dictationFlow.fastHint",
            )}
          </p>
        </div>
      )}
      <div className="dictation-setting">
        <label>
          <strong>{t("dictationFlow.autoPaste")}</strong>
          <input
            type="checkbox"
            checked={settings.auto_paste}
            disabled={disabled || !pasteSupport?.available}
            onChange={(e) => {
              onChange({ ...settings, auto_paste: e.target.checked });
            }}
          />
        </label>
        <p>{t("dictationFlow.pasteHint")}</p>
        <p>
          {t(`dictationFlow.platform.${pasteSupport?.reason ?? "unsupported"}`)}
        </p>
      </div>
      {settings.auto_paste && (
        <>
          <div className="dictation-setting">
            <label>
              <span>{t("dictationFlow.shortcut")}</span>
              <select
                disabled={disabled || pasteSupport?.platform === "macos"}
                value={settings.paste_terminal ? "terminal" : "standard"}
                onChange={(e) => {
                  onChange({
                    ...settings,
                    paste_terminal: e.target.value === "terminal",
                  });
                }}
              >
                <option value="standard">
                  {pasteSupport?.platform === "macos" ? "Command+V" : "Ctrl+V"}
                </option>
                <option value="terminal">Ctrl+Shift+V</option>
              </select>
            </label>
            <p>{t("dictationFlow.terminalHint")}</p>
          </div>
          <div className="dictation-setting">
            <label>
              <span>{t("dictationFlow.delay")}</span>
              <select
                value={settings.paste_delay_ms}
                disabled={disabled}
                onChange={(e) => {
                  onChange({
                    ...settings,
                    paste_delay_ms: Number(e.target.value),
                  });
                }}
              >
                {[150, 250, 500, 1000, 2000].map((ms) => (
                  <option key={ms} value={ms}>
                    {ms} {t("dictationFlow.ms")}
                  </option>
                ))}
              </select>
            </label>
          </div>
        </>
      )}
    </section>
  );
}
