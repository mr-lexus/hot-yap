import { type StatusReport } from "./types";
import Icon from "./Icons";
import { useTranslation } from "react-i18next";

interface TranscriptionProgressProps {
  status: StatusReport;
}

export default function TranscriptionProgress({ status }: TranscriptionProgressProps) {
  const { t } = useTranslation();
  const isTranscribing = status.phase === "transcribing";
  const progress = status.transcribe_progress ?? 0;
  const elapsed = status.transcribe_elapsed;

  if (!isTranscribing) return null;

  const formatTime = (s: number) => {
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${sec.toString().padStart(2, "0")}`;
  };

  return (
    <section className="card card-transcribing">
      <div className="transcribe-heading">
        <span className="progress-spinner"><Icon name="refresh" size={14} /></span>
        <strong>{t("progress.transcribing")}</strong>
        <span>{t("progress.elapsed", { time: formatTime(elapsed) })}</span>
        <b>{Math.round(progress * 100)}%</b>
      </div>
      <div className="transcribe-progress">
        <div className="progress-bar-container">
          <div
            className="progress-bar-fill"
            style={{ width: `${Math.round(progress * 100)}%` }}
          />
        </div>
      </div>
    </section>
  );
}
