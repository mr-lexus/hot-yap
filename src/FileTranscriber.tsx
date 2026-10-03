import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import Icon from "./Icons";
import { providerName } from "./SettingsModal";
import { createSubscriptionScope } from "./subscriptions";
import type { MediaFileInfo, MediaTranscriptionResult, StatusReport } from "./types";

interface FileTranscriberProps {
  open: boolean;
  status: StatusReport;
  onClose: () => void;
  onConfigure: () => void;
}

interface FileProgress {
  stage: "decoding" | "transcribing" | "done";
  fraction: number;
}

const MEDIA_EXTENSIONS = ["wav", "mp3", "m4a", "aac", "flac", "ogg", "oga", "opus", "wma", "mp4", "mov", "m4v", "mkv", "webm"];
const FORMAT_LABELS = ["MP3", "WAV", "M4A", "AAC", "FLAC", "OPUS", "OGG", "WMA", "MP4", "MOV", "M4V", "MKV", "WEBM"];

function fileSize(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function durationLabel(seconds: number): string {
  const total = Math.max(0, Math.round(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(rest).padStart(2, "0")}`
    : `${minutes}:${String(rest).padStart(2, "0")}`;
}

export default function FileTranscriber({ open, status, onClose, onConfigure }: FileTranscriberProps) {
  const { t } = useTranslation();
  const [path, setPath] = useState<string | null>(null);
  const [file, setFile] = useState<MediaFileInfo | null>(null);
  const [result, setResult] = useState<MediaTranscriptionResult | null>(null);
  const [loadingFile, setLoadingFile] = useState(false);
  const [running, setRunning] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [progress, setProgress] = useState<FileProgress>({ stage: "decoding", fraction: 0 });
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [saved, setSaved] = useState(false);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const runningRef = useRef(running);
  runningRef.current = running;

  useEffect(() => {
    if (!open) return;
    previousFocusRef.current = document.activeElement as HTMLElement | null;
    window.requestAnimationFrame(() => closeButtonRef.current?.focus());
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || runningRef.current) return;
      event.preventDefault();
      onClose();
      previousFocusRef.current?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, open]);

  useEffect(() => {
    if (!open) return;
    const subscriptions = createSubscriptionScope((value) => setError(String(value)));
    void subscriptions.add(listen<FileProgress>("vox:file-progress", (event) => {
      setProgress({
        stage: event.payload.stage,
        fraction: Math.min(1, Math.max(0, event.payload.fraction)),
      });
    }));
    void subscriptions.add(getCurrentWindow().onDragDropEvent((event) => {
      if (runningRef.current) return;
      if (event.payload.type === "enter" || event.payload.type === "over") {
        setDragging(true);
      } else if (event.payload.type === "leave") {
        setDragging(false);
      } else if (event.payload.type === "drop") {
        setDragging(false);
        const dropped = event.payload.paths[0];
        if (dropped) void selectFile(dropped);
      }
    }));
    return () => subscriptions.dispose();
  }, [open]);

  if (!open) return null;

  async function selectFile(nextPath: string) {
    setLoadingFile(true);
    setError(null);
    setCopied(false);
    setSaved(false);
    setPath(null);
    setFile(null);
    setResult(null);
    try {
      const info = await invoke<MediaFileInfo>("inspect_media_file", { path: nextPath });
      setPath(nextPath);
      setFile(info);
      setProgress({ stage: "decoding", fraction: 0 });
    } catch (value) {
      setError(String(value));
    } finally {
      setLoadingFile(false);
    }
  }

  async function chooseFile() {
    setError(null);
    try {
      const selected = await openDialog({
        multiple: false,
        directory: false,
        fileAccessMode: "scoped",
        filters: [{ name: t("fileTranscriber.mediaFiles"), extensions: MEDIA_EXTENSIONS }],
      });
      if (typeof selected === "string") await selectFile(selected);
    } catch (value) {
      setError(String(value));
    }
  }

  async function transcribe() {
    if (!path) return;
    setRunning(true);
    setCancelling(false);
    setError(null);
    setResult(null);
    setCopied(false);
    setSaved(false);
    setProgress({ stage: "decoding", fraction: 0 });
    try {
      const value = await invoke<MediaTranscriptionResult>("transcribe_media_file", { path });
      setResult(value);
      setProgress({ stage: "done", fraction: 1 });
    } catch (value) {
      const message = String(value);
      if (!message.includes("Transcription cancelled")) setError(message);
    } finally {
      setRunning(false);
      setCancelling(false);
    }
  }

  async function cancel() {
    setCancelling(true);
    try {
      await invoke("cancel_transcription");
    } catch (value) {
      setError(String(value));
      setCancelling(false);
    }
  }

  async function copyResult() {
    if (!result) return;
    setError(null);
    try {
      await invoke("copy_transcript_text", { text: result.text });
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch (value) {
      setError(String(value));
    }
  }

  async function saveResult() {
    if (!result) return;
    const baseName = result.file_name.replace(/\.[^.]+$/, "") || "transcript";
    setError(null);
    try {
      const destination = await saveDialog({
        defaultPath: `${baseName}.txt`,
        filters: [{ name: t("fileTranscriber.textFiles"), extensions: ["txt"] }],
      });
      if (!destination) return;
      await invoke("save_transcript_file", { path: destination, text: result.text });
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1800);
    } catch (value) {
      setError(String(value));
    }
  }

  const provider = status.stt_provider === "local" ? t("settings.localProvider") : providerName(status.stt_provider);
  const providerReady = status.stt_ready && status.worker_alive;
  const ready = providerReady && status.phase === "idle";

  return (
    <div className="modal-backdrop file-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && !running && onClose()}>
      <section className="file-modal" role="dialog" aria-modal="true" aria-labelledby="file-transcriber-title">
        <header className="modal-header">
          <div>
            <h2 id="file-transcriber-title">{t("fileTranscriber.title")}</h2>
            <p className="modal-subtitle">{t("fileTranscriber.subtitle")}</p>
          </div>
          <button ref={closeButtonRef} className="modal-icon-button" aria-label={t("fileTranscriber.close")} disabled={running} onClick={onClose}><Icon name="close" size={17} /></button>
        </header>

        <div className="file-scroll">
          {!result && (
            <button
              className={`file-dropzone ${dragging ? "dragging" : ""} ${file ? "has-file" : ""}`}
              disabled={running || loadingFile}
              onClick={() => void chooseFile()}
            >
              {file ? (
                <>
                  <span className="file-selected-icon"><Icon name={file.is_video ? "play" : "waveform"} size={22} /></span>
                  <span className="file-selected-copy">
                    <strong>{file.name}</strong>
                    <small>{file.extension.toUpperCase()} · {fileSize(file.size_bytes)}</small>
                  </span>
                  <span className="btn btn-ghost btn-sm">{t("fileTranscriber.changeFile")}</span>
                </>
              ) : (
                <>
                  <span className="file-upload-icon"><Icon name="upload" size={28} /></span>
                  <strong>{loadingFile ? t("fileTranscriber.checking") : dragging ? t("fileTranscriber.dropNow") : t("fileTranscriber.drop")}</strong>
                  <span>{t("fileTranscriber.orChoose")}</span>
                  <span className="btn btn-secondary btn-sm"><Icon name="file" size={13} />{t("fileTranscriber.choose")}</span>
                  <span className="file-format-list">{FORMAT_LABELS.map((format) => <small key={format}>{format}</small>)}</span>
                </>
              )}
            </button>
          )}

          <div className="file-model-row">
            <div className="file-model-copy">
              <span className="panel-icon"><Icon name={status.stt_provider === "local" ? "cpu" : "waveform"} size={15} /></span>
              <div><small>{t("fileTranscriber.model")}</small><strong>{status.stt_model || provider}</strong><span>{provider}</span></div>
            </div>
            <button className="btn btn-ghost btn-sm" disabled={running} onClick={onConfigure}><Icon name="sliders" size={13} />{t("fileTranscriber.changeModel")}</button>
          </div>

          {!ready && (
            <div className="file-readiness-warning">
              <Icon name="help" size={17} />
              <div>
                <strong>{t(providerReady ? "fileTranscriber.appBusy" : "fileTranscriber.notReady")}</strong>
                <span>{providerReady ? t("fileTranscriber.waitUntilIdle") : status.worker_alive ? t("fileTranscriber.configureProvider") : t("fileTranscriber.decoderUnavailable")}</span>
              </div>
              {!providerReady && status.worker_alive && <button className="btn btn-secondary btn-sm" onClick={onConfigure}>{t("fileTranscriber.configure")}</button>}
            </div>
          )}

          {running && (
            <div className="file-progress-card" role="status">
              <div className="file-progress-heading"><span><Icon name="refresh" className="progress-spinner" size={15} />{t(`fileTranscriber.stage.${progress.stage}`)}</span><strong>{Math.round(progress.fraction * 100)}%</strong></div>
              <div className="file-progress-track"><span style={{ width: `${progress.fraction * 100}%` }} /></div>
              <p>{t("fileTranscriber.progressHint")}</p>
              <button className="btn btn-danger btn-sm" disabled={cancelling} onClick={() => void cancel()}>{cancelling ? t("fileTranscriber.cancelling") : t("fileTranscriber.cancel")}</button>
            </div>
          )}

          {result && (
            <div className="file-result">
              <div className="file-result-heading">
                <div><span className="file-result-check"><Icon name="check" size={16} /></span><div><strong>{t("fileTranscriber.ready")}</strong><small>{result.file_name} · {durationLabel(result.duration)}</small></div></div>
                <button className="btn btn-ghost btn-sm" onClick={() => { setPath(null); setFile(null); setResult(null); setCopied(false); setSaved(false); setProgress({ stage: "decoding", fraction: 0 }); }}><Icon name="refresh" size={13} />{t("fileTranscriber.another")}</button>
              </div>
              <textarea value={result.text} onChange={(event) => setResult({ ...result, text: event.target.value })} aria-label={t("fileTranscriber.resultLabel")} />
              <div className="file-result-actions">
                <span>{t("fileTranscriber.characters", { count: result.text.length })}</span>
                <button className="btn btn-secondary" onClick={() => void saveResult()}><Icon name={saved ? "check" : "save"} size={14} />{saved ? t("fileTranscriber.saved") : t("fileTranscriber.save")}</button>
                <button className="btn btn-primary" onClick={() => void copyResult()}><Icon name={copied ? "check" : "clipboard"} size={14} />{copied ? t("fileTranscriber.copied") : t("fileTranscriber.copy")}</button>
              </div>
              {result.warning && <p className="file-result-warning">{result.warning}</p>}
            </div>
          )}

          {error && <p className="history-error file-error" role="alert">{error}</p>}
        </div>

        {!result && !running && (
          <footer className="file-footer">
            <span><Icon name="shield" size={14} />{status.stt_provider === "local" ? t("fileTranscriber.localPrivacy") : t("fileTranscriber.cloudPrivacy", { provider })}</span>
            <button className="btn btn-primary" disabled={!file || !ready || loadingFile} onClick={() => void transcribe()}><Icon name="waveform" size={14} />{t("fileTranscriber.start")}</button>
          </footer>
        )}
      </section>
    </div>
  );
}
