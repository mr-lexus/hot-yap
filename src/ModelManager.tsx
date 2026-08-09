import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { type ModelTier, type StatusReport } from "./types";
import Icon from "./Icons";

interface ModelManagerProps {
  open: boolean;
  onClose: () => void;
  status: StatusReport;
  busy: boolean;
  onRefresh: () => Promise<void>;
}

type StatusFilter = "all" | "downloaded" | "available";
type SortOrder = "recommended" | "name" | "size_asc" | "size_desc" | "popular";

export default function ModelManager({ open, onClose, status, busy, onRefresh }: ModelManagerProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [tierFilter, setTierFilter] = useState<"all" | ModelTier>("all");
  const [familyFilter, setFamilyFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [sortOrder, setSortOrder] = useState<SortOrder>("recommended");
  const [refreshing, setRefreshing] = useState(false);
  const [actionBusy, setActionBusy] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const [catalogCount, setCatalogCount] = useState<number | null>(null);

  useEffect(() => {
    if (!open) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [open, onClose]);

  if (!open) return null;

  const models = status.models;
  const normalizedQuery = query.trim().toLowerCase();
  const families = [...new Set(models.map((model) => model.family))].sort();
  const filteredModels = models.filter((model) => {
    const matchesTier = tierFilter === "all" || model.tier === tierFilter;
    const matchesFamily = familyFilter === "all" || model.family === familyFilter;
    const matchesStatus = statusFilter === "all"
      || (statusFilter === "downloaded" ? model.downloaded : !model.downloaded);
    const searchable = [model.name, model.description, model.family, model.format, model.repo_id, ...model.tags].join(" ").toLowerCase();
    return matchesTier && matchesFamily && matchesStatus && (!normalizedQuery || searchable.includes(normalizedQuery));
  }).sort((left, right) => {
    if (sortOrder === "name") return left.name.localeCompare(right.name);
    if (sortOrder === "size_asc") return left.size_mb - right.size_mb;
    if (sortOrder === "size_desc") return right.size_mb - left.size_mb;
    if (sortOrder === "popular") return (right.downloads ?? 0) - (left.downloads ?? 0);

    const score = (model: typeof left) => {
      if (model.id === status.current_model_id) return -100;
      if (model.downloaded) return -50;
      if (model.family === "Code Switch") return -20;
      if (model.family === "Russian First") return -10;
      return 0;
    };
    return score(left) - score(right) || left.size_mb - right.size_mb;
  });
  const downloadedCount = models.filter((model) => model.downloaded).length;
  const availableCount = models.length - downloadedCount;
  const currentModel = models.find((model) => model.id === status.current_model_id);

  const withAction = async (action: () => Promise<unknown>) => {
    setActionBusy(true);
    try {
      await action();
    } catch (e) {
      console.error("Model action failed:", e);
    } finally {
      setActionBusy(false);
    }
  };

  const handleRefresh = async () => {
    setRefreshing(true);
    try {
      await onRefresh();
    } finally {
      setRefreshing(false);
    }
  };

  const handleDownload = (modelId: string) => withAction(() => invoke("download_model", { model_id: modelId }));
  const handleLoad = (modelId: string) => withAction(() => invoke("load_model", { model_id: modelId }));
  const handleUnload = () => withAction(() => invoke("unload_model"));
  const handleUpdateCatalog = () => withAction(async () => {
    const count = await invoke<number>("update_model_catalog");
    await onRefresh();
    setCatalogCount(count);
  });

  const handleDelete = (modelId: string) => {
    const model = models.find((item) => item.id === modelId);
    if (!confirm(t("models.deleteConfirm", { name: model?.name ?? modelId }))) return;
    return withAction(() => invoke("delete_model", { model_id: modelId }));
  };

  const formatSize = (mb: number) => mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${mb} MB`;
  const disabled = busy || actionBusy;

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}>
      <section className="model-modal" role="dialog" aria-modal="true" aria-labelledby="model-dialog-title">
        <header className="modal-header">
          <div>
            <p className="eyebrow">{t("models.library")}</p>
            <h2 id="model-dialog-title">{t("models.title")}</h2>
            <p className="modal-subtitle">{t("models.subtitle")}</p>
          </div>
          <div className="modal-header-actions">
            <button className={`modal-icon-button ${helpOpen ? "active" : ""}`} onClick={() => setHelpOpen(!helpOpen)} aria-label="Explain model formats"><Icon name="help" size={16} /></button>
            <button className="modal-icon-button" onClick={onClose} aria-label="Close model manager"><Icon name="close" size={17} /></button>
          </div>
        </header>

        {helpOpen && (
          <aside className="model-help">
            <strong>{t("models.help.title")}</strong>
            <p>{t("models.help.families")}</p>
            <p>{t("models.help.weights")}</p>
            <p>{t("models.help.updates")}</p>
          </aside>
        )}

        <div className="model-overview">
          <span>{t("models.readyCount", { downloaded: downloadedCount, total: models.length })}</span>
          {currentModel && <span>{t("models.active", { name: currentModel.name })}</span>}
          <div className="model-overview-actions">
            <button className="text-button" onClick={handleRefresh} disabled={disabled || refreshing}>
              <Icon name="refresh" size={13} />{refreshing ? t("models.refreshing") : t("models.refresh")}
            </button>
            <button className="text-button" onClick={() => void handleUpdateCatalog()} disabled={disabled}>
              <Icon name="search" size={13} />{actionBusy ? t("models.scanning") : t("models.scan")}
            </button>
          </div>
        </div>

        <div className="model-toolbar">
          <label className="model-search">
            <span>{t("models.search")}</span>
            <div className="model-search-field">
              <Icon name="search" size={15} />
              <input autoFocus value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("models.searchPlaceholder")} />
            </div>
          </label>
          <label className="model-select">
            <span>{t("models.family")}</span>
            <select value={familyFilter} onChange={(event) => setFamilyFilter(event.target.value)}>
              <option value="all">{t("models.allFamilies")}</option>
              {families.map((family) => <option key={family} value={family}>{family}</option>)}
            </select>
          </label>
          <label className="model-select">
            <span>{t("models.size")}</span>
            <select value={tierFilter} onChange={(event) => setTierFilter(event.target.value as "all" | ModelTier)}>
              <option value="all">{t("models.allSizes")}</option>
              <option value="light">{t("models.light")}</option>
              <option value="medium">{t("models.medium")}</option>
              <option value="heavy">{t("models.heavy")}</option>
            </select>
          </label>
          <label className="model-select">
            <span>{t("models.sort")}</span>
            <select value={sortOrder} onChange={(event) => setSortOrder(event.target.value as SortOrder)}>
              <option value="recommended">{t("models.recommended")}</option>
              <option value="name">{t("models.name")}</option>
              <option value="size_asc">{t("models.smallFirst")}</option>
              <option value="size_desc">{t("models.largeFirst")}</option>
              <option value="popular">{t("models.popular")}</option>
            </select>
          </label>
        </div>

        <div className="model-filter-bar">
          <div className="segmented-control" aria-label="Installation status filter">
            <button className={statusFilter === "all" ? "active" : ""} onClick={() => setStatusFilter("all")}>{t("models.all")} <span>{models.length}</span></button>
            <button className={statusFilter === "downloaded" ? "active" : ""} onClick={() => setStatusFilter("downloaded")}><Icon name="check" size={12} />{t("models.downloaded")} <span>{downloadedCount}</span></button>
            <button className={statusFilter === "available" ? "active" : ""} onClick={() => setStatusFilter("available")}><Icon name="download" size={12} />{t("models.available")} <span>{availableCount}</span></button>
          </div>
          <span className="model-result-count">{t("models.shown", { count: filteredModels.length })}{catalogCount == null ? "" : ` · ${t("models.catalogChecked", { count: catalogCount })}`}</span>
          {status.model_status === "downloading" && (
            <span className="model-results-progress">
              {status.model_progress == null ? t("model.downloadingUnknown") : t("model.downloading", { progress: Math.round(status.model_progress * 100) })}
            </span>
          )}
        </div>

        <div className="models-list">
          {filteredModels.map((model) => {
            const isCurrent = model.id === status.current_model_id;
            const isDownloading = isCurrent && status.model_status === "downloading";
            const isDownloaded = model.downloaded;
            const isLoaded = model.loaded && status.engine_status === "ready";
            const isLoading = isCurrent && status.engine_status === "loading";
            const canDownload = !isDownloaded && status.model_status !== "downloading" && status.worker_alive;
            const canDelete = isDownloaded && !isLoaded && status.worker_alive;
            const canLoad = isDownloaded && !isLoaded && status.engine_status !== "loading" && status.worker_alive;
            const canUnload = isLoaded && status.engine_status === "ready";

            return (
              <article key={model.id} className={`model-card ${isCurrent ? "current" : ""} ${isLoaded ? "loaded" : ""}`}>
                <div className="model-card-top">
                  <div className="model-title-block">
                    <div className="model-title-line">
                      <h3>{model.name}</h3>
                      {isCurrent && <span className="badge current-badge">{t("models.selected")}</span>}
                      {isLoaded && <span className="badge loaded-badge">{t("models.loaded")}</span>}
                    </div>
                    <div className="model-tags">{model.tags.slice(0, 4).map((tag) => <span key={tag} className="model-tag">{tag}</span>)}</div>
                  </div>
                  <span className={`tier-badge tier-${model.tier}`}>{t(`models.${model.tier}`)}</span>
                </div>
                <p className="model-description">{model.description}</p>
                <div className="model-source-line"><span>{model.family}</span><span>{model.format}</span><span>{model.repo_id}</span></div>
                <div className="model-card-bottom">
                  <div className="model-meta">
                    <span className="model-size">{formatSize(model.size_mb)}</span>
                    {isDownloading && <span className="model-status downloading">{status.model_progress == null ? t("model.downloadingUnknown") : t("model.downloading", { progress: Math.round(status.model_progress * 100) })}</span>}
                    {isDownloaded && !isLoaded && !isDownloading && <span className="model-status downloaded">{t("models.downloaded")}</span>}
                    {isLoading && <span className="model-status loading">{t("models.loading")}</span>}
                    {isLoaded && <span className="model-status loaded">{t("model.ready")}</span>}
                    {!isDownloaded && !isDownloading && status.model_status !== "error" && <span className="model-status not-downloaded">{t("models.notDownloaded")}</span>}
                    {isCurrent && status.model_status === "error" && <span className="model-status error">{status.model_error ?? t("models.downloadFailed")}</span>}
                  </div>
                  <div className="model-actions">
                    {canDownload && <button className="btn btn-primary btn-sm" onClick={() => handleDownload(model.id)} disabled={disabled}><Icon name="download" size={13} />{t("models.download")}</button>}
                    {canLoad && <button className="btn btn-primary btn-sm" onClick={() => handleLoad(model.id)} disabled={disabled || isLoading}><Icon name="play" size={13} />{isLoading ? t("models.loading") : t("models.load")}</button>}
                    {canUnload && <button className="btn btn-ghost btn-sm" onClick={handleUnload} disabled={disabled}><Icon name="stop" size={13} />{t("models.unload")}</button>}
                    {canDelete && <button className="btn btn-danger btn-sm" onClick={() => void handleDelete(model.id)} disabled={disabled}><Icon name="trash" size={13} />{t("models.delete")}</button>}
                    {isLoaded && !canUnload && <button className="btn btn-ghost btn-sm" disabled>{t("models.inUse")}</button>}
                  </div>
                </div>
              </article>
            );
          })}
          {filteredModels.length === 0 && <p className="empty-filter">{t("models.noMatches")}</p>}
        </div>
      </section>
    </div>
  );
}
