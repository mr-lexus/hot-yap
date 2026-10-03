import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import Icon from "./Icons";
import type { DictionaryData, DictionaryEntry } from "./types";
import "./dictionary.css";

interface Props {
  open: boolean;
  onClose: () => void;
  onChanged: (data: DictionaryData) => void;
}
interface Scan {
  terms: string[];
  files: number;
  truncated: boolean;
}

export default function DictionaryModal({ open, onClose, onChanged }: Props) {
  const { t } = useTranslation();
  const [data, setData] = useState<DictionaryData | null>(null);
  const [scope, setScope] = useState<string | null>(null);
  const [tab, setTab] = useState<"entries" | "suggestions">("entries");
  const [query, setQuery] = useState("");
  const [heard, setHeard] = useState("");
  const [written, setWritten] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [scan, setScan] = useState<Scan | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [projectName, setProjectName] = useState("");
  const [addingProject, setAddingProject] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const dialogRef = useRef<HTMLElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const busyRef = useRef(false);
  busyRef.current = busy;
  const closeAction = useRef(onClose);
  closeAction.current = onClose;

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setError(null);
    setScan(null);
    setData(null);
    setHeard("");
    setWritten("");
    setEditing(null);
    setAddingProject(false);
    setConfirmDelete(false);
    setQuery("");
    setTab("entries");
    void invoke<DictionaryData>("get_dictionary")
      .then((value) => {
        if (!cancelled) {
          setData(value);
          setScope(value.active_project);
        }
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    const previous = document.activeElement as HTMLElement | null;
    closeRef.current?.focus();
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busyRef.current) {
        e.preventDefault();
        closeAction.current();
      }
      if (e.key === "Tab") {
        const items = Array.from(
          dialogRef.current?.querySelectorAll<HTMLElement>(
            'button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex="0"]',
          ) ?? [],
        ).filter((el) => el.offsetParent !== null);
        const first = items[0],
          last = items[items.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last?.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first?.focus();
        }
      }
    };
    window.addEventListener("keydown", key);
    return () => {
      cancelled = true;
      window.removeEventListener("keydown", key);
      previous?.focus();
    };
  }, [open]);

  if (!open) return null;
  const save = async (next: DictionaryData) => {
    const saved = await invoke<DictionaryData>("save_dictionary", {
      dictionary: next,
    });
    setData(saved);
    onChanged(saved);
    return saved;
  };
  const run = async (action: () => Promise<unknown>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      busyRef.current = false;
    }
  };
  const changeScope = (id: string | null) => {
    setTab("entries");
    setScope(id);
    setScan(null);
    setSelected(new Set());
    setEditing(null);
    setHeard("");
    setWritten("");
    setQuery("");
    setConfirmDelete(false);
  };
  const project = data?.projects.find((p) => p.id === scope);
  const entries =
    (tab === "entries" ? data?.entries : data?.suggestions)?.filter(
      (e) =>
        e.project_id === scope &&
        `${e.heard} ${e.written}`
          .toLocaleLowerCase()
          .includes(query.toLocaleLowerCase()),
    ) ?? [];
  const suggestions =
    data?.suggestions.filter((e) => e.project_id === scope).length ?? 0;
  const scanFolder = async (path: string) => {
    setScan(await invoke<Scan>("scan_project", { path }));
    setSelected(new Set());
  };
  const addProject = async () => {
    if (!data) return;
    const path = await openDialog({
      directory: true,
      multiple: false,
      title: t("dictionary.chooseFolder"),
    });
    if (typeof path !== "string") return;
    const existing = data.projects.find((p) => p.path === path);
    if (existing) {
      changeScope(existing.id);
      return;
    }
    const id = crypto.randomUUID();
    const name =
      projectName.trim() ||
      path
        .replace(/[\\/]+$/, "")
        .split(/[\\/]/)
        .slice(-1)[0] ||
      t("dictionary.project");
    await save({ ...data, projects: [...data.projects, { id, name, path }] });
    changeScope(id);
    setAddingProject(false);
    setProjectName("");
    await scanFolder(path);
  };
  const submitEntry = async () => {
    if (!data || !written.trim()) return;
    const existing = data.entries.find((e) => e.id === editing);
    const entry: DictionaryEntry = {
      id: editing ?? crypto.randomUUID(),
      heard: heard.trim(),
      written: written.trim(),
      project_id: scope,
      origin: existing?.origin ?? "manual",
      enabled: existing?.enabled ?? true,
    };
    await save({
      ...data,
      entries: editing
        ? data.entries.map((e) => (e.id === editing ? entry : e))
        : [...data.entries, entry],
    });
    setEditing(null);
    setHeard("");
    setWritten("");
  };

  return (
    <div
      className="modal-backdrop dictionary-backdrop"
      onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}
    >
      <section
        ref={dialogRef}
        className="dictionary-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="dictionary-title"
      >
        <header className="modal-header">
          <div>
            <div className="dictionary-eyebrow">
              <Icon name="book" size={14} />
              {t("dictionary.eyebrow")}
            </div>
            <h2 id="dictionary-title">{t("dictionary.title")}</h2>
            <p className="modal-subtitle">{t("dictionary.subtitle")}</p>
          </div>
          <button
            ref={closeRef}
            className="modal-icon-button"
            disabled={busy}
            onClick={onClose}
            aria-label={t("settings.close")}
          >
            <Icon name="close" />
          </button>
        </header>
        {data ? (
          <div className="dictionary-layout" aria-busy={busy}>
            <nav
              className="dictionary-sidebar"
              aria-label={t("dictionary.scopes")}
            >
              <button
                className={`dictionary-scope ${scope === null ? "selected" : ""}`}
                disabled={busy}
                onClick={() => changeScope(null)}
              >
                <Icon name="book" />
                <span>{t("dictionary.personal")}</span>
                <b>{data.entries.filter((e) => !e.project_id).length}</b>
              </button>
              <div className="dictionary-sidebar-label">
                {t("dictionary.projects")}
              </div>
              {data.projects.map((p) => (
                <button
                  key={p.id}
                  className={`dictionary-scope ${scope === p.id ? "selected" : ""}`}
                  disabled={busy}
                  onClick={() => changeScope(p.id)}
                >
                  <Icon name="folder" />
                  <span>{p.name}</span>
                  {data.active_project === p.id && (
                    <span
                      className="dictionary-active-dot"
                      title={t("dictionary.active")}
                    />
                  )}
                </button>
              ))}
              <button
                className="dictionary-add-project"
                disabled={busy}
                onClick={() => setAddingProject(!addingProject)}
              >
                <Icon name="plus" size={14} />
                {t("dictionary.addProject")}
              </button>
              {addingProject && (
                <form
                  className="dictionary-project-form"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void run(addProject);
                  }}
                >
                  <input
                    autoFocus
                    maxLength={100}
                    value={projectName}
                    onChange={(e) => setProjectName(e.target.value)}
                    placeholder={t("dictionary.projectName")}
                    aria-label={t("dictionary.projectName")}
                  />
                  <button className="btn btn-secondary btn-sm" disabled={busy}>
                    {t("dictionary.chooseFolder")}
                  </button>
                </form>
              )}
              <div className="dictionary-learning">
                <Icon name="sparkles" size={18} />
                <strong>{t("dictionary.learning")}</strong>
                <p>{t("dictionary.learningHint")}</p>
                <label>
                  <span className="sr-only">{t("dictionary.learning")}</span>
                  <select
                    value={data.learning}
                    disabled={busy}
                    onChange={(e) =>
                      void run(() =>
                        save({
                          ...data,
                          learning: e.target
                            .value as DictionaryData["learning"],
                        }),
                      )
                    }
                  >
                    <option value="suggest">
                      {t("dictionary.learningSuggest")}
                    </option>
                    <option value="auto">{t("dictionary.learningAuto")}</option>
                    <option value="off">{t("dictionary.learningOff")}</option>
                  </select>
                </label>
              </div>
            </nav>
            <div className="dictionary-main">
              <div className="dictionary-scope-header">
                <div>
                  <h3>{project?.name ?? t("dictionary.personal")}</h3>
                  <p>
                    {project
                      ? t("dictionary.projectHint")
                      : t("dictionary.personalHint")}
                  </p>
                </div>
                <button
                  className={`btn btn-sm ${data.active_project === scope ? "btn-secondary" : "btn-primary"}`}
                  disabled={busy || data.active_project === scope}
                  onClick={() =>
                    void run(() => save({ ...data, active_project: scope }))
                  }
                >
                  <Icon name="check" size={13} />
                  {t(
                    data.active_project === scope
                      ? "dictionary.active"
                      : "dictionary.activate",
                  )}
                </button>
              </div>
              {project && (
                <div className="dictionary-project-path">
                  <span title={project.path}>{project.path}</span>
                  <button
                    className="btn btn-ghost btn-sm"
                    disabled={busy}
                    onClick={() => void run(() => scanFolder(project.path))}
                  >
                    <Icon name="refresh" size={13} />
                    {t("dictionary.scan")}
                  </button>
                  <button
                    className="modal-icon-button"
                    disabled={busy}
                    aria-label={t("dictionary.deleteProject")}
                    onClick={() => setConfirmDelete(!confirmDelete)}
                  >
                    <Icon name="trash" size={14} />
                  </button>
                </div>
              )}
              {confirmDelete && project && (
                <div className="dictionary-notice">
                  <span>{t("dictionary.deleteProjectHint")}</span>
                  <button
                    className="btn btn-danger btn-sm"
                    disabled={busy}
                    onClick={() =>
                      void run(async () => {
                        await save({
                          ...data,
                          projects: data.projects.filter((p) => p.id !== scope),
                          entries: data.entries.filter(
                            (e) => e.project_id !== scope,
                          ),
                          suggestions: data.suggestions.filter(
                            (e) => e.project_id !== scope,
                          ),
                          active_project:
                            data.active_project === scope
                              ? null
                              : data.active_project,
                        });
                        changeScope(null);
                      })
                    }
                  >
                    {t("history.delete")}
                  </button>
                  <button
                    className="btn btn-ghost btn-sm"
                    onClick={() => setConfirmDelete(false)}
                  >
                    {t("history.cancel")}
                  </button>
                </div>
              )}
              {scan && (
                <section className="dictionary-scan">
                  <div className="dictionary-scan-heading">
                    <div>
                      <strong>
                        {t("dictionary.scanTitle", {
                          count: scan.terms.length,
                        })}
                      </strong>
                      <p>
                        {t("dictionary.scanHint", { count: scan.files })}
                        {scan.truncated && ` ${t("dictionary.scanLimited")}`}
                      </p>
                    </div>
                    <button
                      className="modal-icon-button"
                      onClick={() => setScan(null)}
                      aria-label={t("settings.close")}
                    >
                      <Icon name="close" size={14} />
                    </button>
                  </div>
                  <div className="dictionary-term-cloud">
                    {scan.terms.map((term) => {
                      const exists = data.entries.some(
                        (e) => e.project_id === scope && e.written === term,
                      );
                      return (
                        <label
                          key={term}
                          className={`${selected.has(term) ? "selected" : ""} ${exists ? "exists" : ""}`}
                        >
                          <input
                            type="checkbox"
                            disabled={busy || exists}
                            checked={exists || selected.has(term)}
                            onChange={(e) => {
                              const next = new Set(selected);
                              e.target.checked
                                ? next.add(term)
                                : next.delete(term);
                              setSelected(next);
                            }}
                          />
                          <span>{term}</span>
                        </label>
                      );
                    })}
                  </div>
                  <div className="dictionary-scan-actions">
                    <button
                      className="btn btn-ghost btn-sm"
                      disabled={busy}
                      onClick={() =>
                        setSelected(
                          new Set(
                            scan.terms.filter(
                              (term) =>
                                !data.entries.some(
                                  (e) =>
                                    e.project_id === scope &&
                                    e.written === term,
                                ),
                            ),
                          ),
                        )
                      }
                    >
                      {t("dictionary.selectAll")}
                    </button>
                    <button
                      className="btn btn-primary btn-sm"
                      disabled={busy || !selected.size}
                      onClick={() =>
                        void run(async () => {
                          await save({
                            ...data,
                            entries: [
                              ...data.entries,
                              ...Array.from(selected).map((written) => ({
                                id: crypto.randomUUID(),
                                heard: "",
                                written,
                                project_id: scope,
                                origin: "project" as const,
                                enabled: true,
                              })),
                            ],
                          });
                          setScan(null);
                        })
                      }
                    >
                      {t("dictionary.import", { count: selected.size })}
                    </button>
                  </div>
                </section>
              )}
              <div className="dictionary-toolbar">
                <div className="segmented-control">
                  <button
                    className={tab === "entries" ? "active" : ""}
                    onClick={() => setTab("entries")}
                  >
                    {t("dictionary.entries")}
                  </button>
                  <button
                    className={tab === "suggestions" ? "active" : ""}
                    onClick={() => setTab("suggestions")}
                  >
                    {t("dictionary.suggestions")}
                    <span>{suggestions}</span>
                  </button>
                </div>
                <label className="model-search">
                  <Icon name="search" size={14} />
                  <input
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    placeholder={t("dictionary.search")}
                    aria-label={t("dictionary.search")}
                  />
                </label>
              </div>
              {tab === "entries" && (
                <form
                  className="dictionary-entry-form"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void run(submitEntry);
                  }}
                >
                  <label>
                    {t("dictionary.heard")}
                    <input
                      value={heard}
                      maxLength={120}
                      disabled={busy}
                      onChange={(e) => setHeard(e.target.value)}
                      placeholder={t("dictionary.heardPlaceholder")}
                    />
                  </label>
                  <Icon name="arrow" size={16} />
                  <label>
                    {t("dictionary.written")}
                    <input
                      value={written}
                      maxLength={120}
                      disabled={busy}
                      onChange={(e) => setWritten(e.target.value)}
                      placeholder="Supabase"
                      required
                    />
                  </label>
                  <button
                    className="btn btn-primary"
                    disabled={busy || !written.trim()}
                  >
                    <Icon name={editing ? "check" : "plus"} size={15} />
                    {t(editing ? "settings.save" : "dictionary.add")}
                  </button>
                  {editing && (
                    <button
                      type="button"
                      className="modal-icon-button"
                      aria-label={t("history.cancel")}
                      onClick={() => {
                        setEditing(null);
                        setHeard("");
                        setWritten("");
                      }}
                    >
                      <Icon name="close" size={15} />
                    </button>
                  )}
                  <p>{t("dictionary.entryHint")}</p>
                </form>
              )}
              <div className="dictionary-entries">
                {entries.length ? (
                  entries.map((entry) => (
                    <article
                      key={entry.id}
                      className={`dictionary-entry ${entry.enabled ? "" : "disabled"}`}
                    >
                      <div className="dictionary-entry-words">
                        <span>{entry.heard || t("dictionary.term")}</span>
                        <Icon name="arrow" size={13} />
                        <strong>{entry.written}</strong>
                      </div>
                      <span className="dictionary-origin">
                        {t(`dictionary.origin.${entry.origin}`)}
                      </span>
                      {tab === "suggestions" ? (
                        <button
                          className="btn btn-secondary btn-sm"
                          disabled={busy}
                          onClick={() =>
                            void run(() =>
                              save({
                                ...data,
                                entries: [...data.entries, entry],
                                suggestions: data.suggestions.filter(
                                  (e) => e.id !== entry.id,
                                ),
                              }),
                            )
                          }
                        >
                          {t("dictionary.accept")}
                        </button>
                      ) : (
                        <>
                          <button
                            className="modal-icon-button"
                            disabled={busy}
                            aria-label={t("dictionary.edit")}
                            onClick={() => {
                              setEditing(entry.id);
                              setHeard(entry.heard);
                              setWritten(entry.written);
                            }}
                          >
                            <Icon name="edit" size={14} />
                          </button>
                          <label
                            className="dictionary-enable"
                            title={t("dictionary.enabled")}
                          >
                            <input
                              type="checkbox"
                              aria-label={t("dictionary.enabled")}
                              checked={entry.enabled}
                              disabled={busy}
                              onChange={() =>
                                void run(() =>
                                  save({
                                    ...data,
                                    entries: data.entries.map((e) =>
                                      e.id === entry.id
                                        ? { ...e, enabled: !e.enabled }
                                        : e,
                                    ),
                                  }),
                                )
                              }
                            />
                          </label>
                        </>
                      )}
                      <button
                        className="modal-icon-button"
                        disabled={busy}
                        aria-label={t("history.delete")}
                        onClick={() =>
                          void run(() =>
                            save({
                              ...data,
                              [tab === "entries" ? "entries" : "suggestions"]:
                                (tab === "entries"
                                  ? data.entries
                                  : data.suggestions
                                ).filter((e) => e.id !== entry.id),
                            }),
                          )
                        }
                      >
                        <Icon name="trash" size={14} />
                      </button>
                    </article>
                  ))
                ) : (
                  <div className="dictionary-empty">
                    <Icon
                      name={tab === "suggestions" ? "sparkles" : "book"}
                      size={30}
                    />
                    <strong>
                      {t(
                        query
                          ? "history.noMatches"
                          : tab === "suggestions"
                            ? "dictionary.noSuggestions"
                            : "dictionary.empty",
                      )}
                    </strong>
                    <p>
                      {t(
                        tab === "suggestions"
                          ? "dictionary.noSuggestionsHint"
                          : "dictionary.emptyHint",
                      )}
                    </p>
                  </div>
                )}
              </div>
            </div>
          </div>
        ) : (
          <p className="dictionary-loading">{t("settings.loading")}</p>
        )}
        <footer className="dictionary-footer">
          <span>
            <Icon name="lock" size={13} />
            {t("dictionary.local")}
          </span>
          <span role="status">
            {busy ? t("dictionary.working") : t("dictionary.saved")}
          </span>
        </footer>
        {error && (
          <div className="dictionary-error" role="alert">
            {error}
            <button
              className="btn btn-ghost btn-sm"
              onClick={() =>
                void run(async () => {
                  const value = await invoke<DictionaryData>("get_dictionary");
                  setData(value);
                  onChanged(value);
                })
              }
            >
              {t("dictionary.reload")}
            </button>
          </div>
        )}
      </section>
    </div>
  );
}
