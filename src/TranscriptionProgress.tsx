import { type StatusReport } from "./types";
import Icon from "./Icons";
import { useTranslation } from "react-i18next";

interface TranscriptionProgressProps {
  status: StatusReport;
  onCancel?: () => void;
}

export default function TranscriptionProgress({ status, onCancel }: TranscriptionProgressProps) {
  const { t } = useTranslation();
  const isTranscribing = status.phase === "transcribing";
  const elapsed = status.transcribe_elapsed;

  if (!isTranscribing) return null;

  const formatTime = (s: number) => {
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${sec.toString().padStart(2, "0")}`;
  };

  return (
    <span className="transcribe-heading">
      <span className="progress-spinner"><Icon name="refresh" size={13} /></span>
      <span>{t("progress.transcribing")}</span>
      <span>{t("progress.elapsed", { time: formatTime(elapsed) })}</span>
      {onCancel && (
        <button
          className="btn-cancel-transcribe"
          onClick={onCancel}
          title={t("progress.cancel")}
          aria-label={t("progress.cancel")}
        >
          <Icon name="close" size={12} />
        </button>
      )}
    </span>
  );
}
