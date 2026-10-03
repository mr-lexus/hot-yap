import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import Icon from "./Icons";
import { providerName } from "./SettingsModal";
import { createSubscriptionScope } from "./subscriptions";
import type { HistoryEntry } from "./types";

interface HistoryModalProps {
  open: boolean;
  historyEnabled: boolean;
  onClose: () => void;
  onOpenSettings: () => void;
}

type HistoryFilter = "all" | "favorites";

export default function HistoryModal({ open, historyEnabled, onClose, onOpenSettings }: HistoryModalProps) {
  const { t, i18n } = useTranslation();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [filter, setFilter] = useState<HistoryFilter>("all");
  const [provider, setProvider] = useState("all");
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null);
  const [clearConfirm, setClearConfirm] = useState(false);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const busyRef = useRef(false);
  busyRef.current = activeId !== null;

  useEffect(() => {
    if (!open) return;
    let ignore = false;
    setLoading(true);
    setError(null);
    setPendingDeleteId(null);
    setClearConfirm(false);
    void invoke<HistoryEntry[]>("get_history")
      .then((value) => { if (!ignore) setEntries(value); })
      .catch((value) => { if (!ignore) setError(String(value)); })
      .finally(() => { if (!ignore) setLoading(false); });
    return () => { ignore = true; };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const subscriptions = createSubscriptionScope((value) => setError(String(value)));
    void subscriptions.add(listen<HistoryEntry>("vox:history-added", (event) => {
      setEntries((current) => [event.payload, ...current.filter((entry) => entry.id !== event.payload.id)]);
    }));
    return () => subscriptions.dispose();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    previousFocusRef.current = document.activeElement as HTMLElement | null;
    window.requestAnimationFrame(() => closeButtonRef.current?.focus());
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || busyRef.current) return;
      event.preventDefault();
      onClose();
      previousFocusRef.current?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, open]);

  const providers = useMemo(
    () => [...new Set(entries.map((entry) => entry.provider))].sort((a, b) => providerName(a).localeCompare(providerName(b))),
    [entries],
  );
  const visibleEntries = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    return entries.filter((entry) => {
      if (filter === "favorites" && !entry.favorite) return false;
      if (provider !== "all" && entry.provider !== provider) return false;
      if (!normalizedQuery) return true;
      return `${entry.text}\n${entry.model}\n${providerName(entry.provider)}`.toLocaleLowerCase().includes(normalizedQuery);
    });
  }, [entries, filter, provider, query]);
  const dateFormatter = useMemo(
    () => new Intl.DateTimeFormat(i18n.resolvedLanguage, { dateStyle: "medium", timeStyle: "short" }),
    [i18n.resolvedLanguage],
  );
  const favoriteCount = entries.filter((entry) => entry.favorite).length;
  const nonFavoriteCount = entries.length - favoriteCount;

  useEffect(() => {
    if (provider !== "all" && !providers.includes(provider)) setProvider("all");
  }, [provider, providers]);

  if (!open) return null;

  const toggleFavorite = async (entry: HistoryEntry) => {
    setActiveId(entry.id);
    setError(null);
    try {
      const updated = await invoke<HistoryEntry>("set_history_favorite", { id: entry.id, favorite: !entry.favorite });
      setEntries((current) => current.map((item) => item.id === updated.id ? updated : item));
    } catch (value) {
      setError(String(value));
    } finally {
      setActiveId(null);
    }
  };

  const copy = async (id: string) => {
    setActiveId(id);
    setError(null);
    try {
      await invoke("copy_history_entry", { id });
      setCopiedId(id);
      window.setTimeout(() => setCopiedId((current) => current === id ? null : current), 1600);
    } catch (value) {
      setError(String(value));
    } finally {
      setActiveId(null);
    }
  };

  const deleteEntry = async (id: string) => {
    setActiveId(id);
    setError(null);
    try {
      await invoke("delete_history_entry", { id });
      setEntries((current) => current.filter((entry) => entry.id !== id));
      setPendingDeleteId(null);
    } catch (value) {
      setError(String(value));
    } finally {
      setActiveId(null);
    }
  };

  const clear = async (keepFavorites: boolean) => {
    setActiveId("clear");
    setError(null);
    try {
      await invoke<number>("clear_history", { keep_favorites: keepFavorites });
      setEntries((current) => keepFavorites ? current.filter((entry) => entry.favorite) : []);
      setClearConfirm(false);
    } catch (value) {
      setError(String(value));
    } finally {
      setActiveId(null);
    }
  };

  return (
    <div className="modal-backdrop history-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && !activeId && onClose()}>
      <section className="history-modal" role="dialog" aria-modal="true" aria-labelledby="history-title">
        <header className="modal-header">
          <div>
            <div className="history-title-row">
              <h2 id="history-title">{t("history.title")}</h2>
              <span className={`history-state ${historyEnabled ? "enabled" : ""}`}>{t(historyEnabled ? "history.on" : "history.off")}</span>
            </div>
            <p className="modal-subtitle">{t("history.subtitle")}</p>
          </div>
          <button ref={closeButtonRef} className="modal-icon-button" aria-label={t("history.close")} disabled={activeId !== null} onClick={onClose}><Icon name="close" size={17} /></button>
        </header>

        {!historyEnabled && (
          <div className="history-disabled-note">
            <Icon name="lock" size={16} />
            <span>{t("history.disabledHint")}</span>
            <button className="btn btn-secondary btn-sm" onClick={onOpenSettings}><Icon name="sliders" size={13} />{t("history.openSettings")}</button>
          </div>
        )}

        <div className="history-toolbar">
          <label className="model-search history-search">
            <Icon name="search" size={14} />
            <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("history.search")} aria-label={t("history.search")} />
          </label>
          <div className="segmented-control" aria-label={t("history.filter")}>
            <button className={filter === "all" ? "active" : ""} onClick={() => setFilter("all")}>{t("history.all")}<span>{entries.length}</span></button>
            <button className={filter === "favorites" ? "active" : ""} onClick={() => setFilter("favorites")}><Icon name="star" size={12} />{t("history.favorites")}<span>{favoriteCount}</span></button>
          </div>
          <label className="history-provider-filter">
            <span className="sr-only">{t("history.provider")}</span>
            <select value={provider} onChange={(event) => setProvider(event.target.value)} aria-label={t("history.provider")}>
              <option value="all">{t("history.allProviders")}</option>
              {providers.map((id) => <option key={id} value={id}>{providerName(id)}</option>)}
            </select>
          </label>
        </div>

        <div className="history-content" aria-busy={loading}>
          {loading ? (
            <div className="history-empty"><Icon name="refresh" className="progress-spinner" size={20} /><p>{t("history.loading")}</p></div>
          ) : visibleEntries.length === 0 ? (
            <div className="history-empty">
              <Icon name={entries.length === 0 ? "history" : "search"} size={26} />
              <h3>{t(entries.length === 0 ? "history.empty" : "history.noMatches")}</h3>
              <p>{t(entries.length === 0 ? (historyEnabled ? "history.emptyHint" : "history.emptyDisabledHint") : "history.noMatchesHint")}</p>
            </div>
          ) : (
            <div className="history-list">
              {visibleEntries.map((entry) => (
                <article key={entry.id} className={`history-entry ${entry.favorite ? "favorite" : ""}`}>
                  <div className="history-entry-meta">
                    <time dateTime={new Date(entry.created_at).toISOString()}>{dateFormatter.format(entry.created_at)}</time>
                    {entry.source_name && <span title={entry.source_name}>{entry.source_name}</span>}
                    <span>{providerName(entry.provider)}</span>
                    {entry.model && <span title={entry.model}>{entry.model}</span>}
                  </div>
                  <p>{entry.text}</p>
                  <div className="history-entry-actions">
                    {pendingDeleteId === entry.id ? (
                      <>
                        <span>{t("history.deleteConfirm")}</span>
                        <button className="btn btn-danger btn-sm" disabled={activeId !== null} onClick={() => void deleteEntry(entry.id)}>{t("history.delete")}</button>
                        <button className="btn btn-ghost btn-sm" disabled={activeId !== null} onClick={() => setPendingDeleteId(null)}>{t("history.cancel")}</button>
                      </>
                    ) : (
                      <>
                        <button className={`history-action ${entry.favorite ? "active" : ""}`} disabled={activeId !== null} title={t(entry.favorite ? "history.unfavorite" : "history.favorite")} aria-label={t(entry.favorite ? "history.unfavorite" : "history.favorite")} onClick={() => void toggleFavorite(entry)}><Icon name="star" size={15} /></button>
                        <button className="history-action" disabled={activeId !== null} title={t("history.copy")} aria-label={t("history.copy")} onClick={() => void copy(entry.id)}><Icon name={copiedId === entry.id ? "check" : "clipboard"} size={15} /></button>
                        <button className="history-action danger" disabled={activeId !== null} title={t("history.delete")} aria-label={t("history.delete")} onClick={() => setPendingDeleteId(entry.id)}><Icon name="trash" size={15} /></button>
                      </>
                    )}
                  </div>
                </article>
              ))}
            </div>
          )}
        </div>

        <footer className="history-footer">
          <span>{t("history.shown", { shown: visibleEntries.length, total: entries.length })}</span>
          {clearConfirm ? (
            <div className="history-clear-confirm">
              <span>{t("history.clearConfirm")}</span>
              <button className="btn btn-ghost btn-sm" disabled={activeId !== null} onClick={() => setClearConfirm(false)}>{t("history.cancel")}</button>
              {nonFavoriteCount > 0 && favoriteCount > 0 && <button className="btn btn-secondary btn-sm" disabled={activeId !== null} onClick={() => void clear(true)}>{t("history.clearExceptFavorites")}</button>}
              <button className="btn btn-danger btn-sm" disabled={activeId !== null || entries.length === 0} onClick={() => void clear(false)}>{t("history.clearAll")}</button>
            </div>
          ) : (
            <button className="btn btn-ghost btn-sm" disabled={activeId !== null || entries.length === 0} onClick={() => setClearConfirm(true)}><Icon name="trash" size={13} />{t("history.clear")}</button>
          )}
        </footer>
        {error && <p className="history-error" role="alert">{error}</p>}
      </section>
    </div>
  );
}
