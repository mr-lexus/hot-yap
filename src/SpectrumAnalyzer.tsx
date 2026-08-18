import { useTranslation } from "react-i18next";

interface SpectrumAnalyzerProps {
  audioLevel: number;
  spectrum: number[];
  isRecording: boolean;
}

export default function SpectrumAnalyzer({ audioLevel, spectrum, isRecording }: SpectrumAnalyzerProps) {
  const { t } = useTranslation();
  const BAR_COUNT = 32;
  const bars = spectrum.length >= BAR_COUNT ? spectrum : Array(BAR_COUNT).fill(0);
  const level = isRecording ? Math.min(1, audioLevel * 4) : 0;

  return (
    <div className={`spectrum-analyzer ${isRecording ? "" : "idle"}`}>
      <div className="spectrum-bars" aria-label="Live microphone activity">
        {bars.slice(0, BAR_COUNT).map((value, index) => {
          const height = Math.max(3, Math.min(100, Math.pow(Math.max(value, 0), 0.65) * 100));
          return <span key={index} className="spectrum-bar" style={{ height: `${height}%` }} />;
        })}
      </div>
      <div className="spectrum-level-track">
        <span className="spectrum-level" style={{ width: `${level * 100}%` }} />
      </div>
      <div className="spectrum-labels">
        <span>{t("spectrum.quiet")}</span>
        <span>{t("spectrum.voice")}</span>
        <span>{t("spectrum.loud")}</span>
      </div>
    </div>
  );
}

