"""Model download / status helpers for the HotYap worker.

The Rust side owns the model catalog (id, repo_id, allow_patterns, optional
CT2 subdir). The worker only deals with directories under the app model root:

    <root>/<model_id>/model.bin            (repos with files at root)
    <root>/<model_id>/ct2_int8_float16/    (repos with a CT2 subfolder)
"""

import shutil
import sys
from pathlib import Path

MIN_MODEL_BYTES = 1_000_000


def log(*a):
    print(*a, file=sys.stderr, flush=True)


def _model_path(root, model_id: str, ct2_subdir):
    p = Path(root) / model_id
    return (p / ct2_subdir) if ct2_subdir else p


def is_downloaded(root, model_id: str, ct2_subdir=None) -> bool:
    if not root:
        return False
    p = _model_path(root, model_id, ct2_subdir) / "model.bin"
    try:
        return p.exists() and p.stat().st_size > MIN_MODEL_BYTES
    except OSError:
        return False


def model_status(root, model_id: str, ct2_subdir=None) -> str:
    return "downloaded" if is_downloaded(root, model_id, ct2_subdir) else "missing"


def disk_bytes(root, model_id: str, ct2_subdir=None) -> int:
    d = _model_path(root, model_id, ct2_subdir)
    if not d.is_dir():
        return 0
    try:
        return sum(f.stat().st_size for f in d.rglob("*") if f.is_file())
    except OSError:
        return 0


def delete_model(root, model_id: str) -> None:
    p = Path(root) / model_id
    if p.is_dir():
        shutil.rmtree(p)
        log(f"deleted model '{model_id}' ({p})")


def _expected_total(root, repo_id: str, model_id: str, ct2_subdir, revision=None):
    try:
        from huggingface_hub import HfApi

        path_in_repo = ct2_subdir or "."
        files = HfApi().list_repo_tree(
            repo_id,
            path_in_repo=path_in_repo,
            repo_type="model",
            recursive=True,
            revision=revision,
        )
        total = sum(getattr(f, "size", None) or 0 for f in files)
        return total or None
    except Exception as e:
        log(f"could not fetch expected download size from HF API: {e}")
        return None


def download_model(root, model_id: str, repo_id: str, allow_patterns, ct2_subdir, revision=None, on_progress=None) -> None:
    root = Path(root)
    if is_downloaded(root, model_id, ct2_subdir):
        log(f"model '{model_id}' already downloaded, skipping")
        if on_progress:
            on_progress(1.0)
        return

    model_root = root / model_id
    model_root.mkdir(parents=True, exist_ok=True)
    total = _expected_total(root, repo_id, model_id, ct2_subdir, revision)

    import tqdm
    from huggingface_hub import snapshot_download

    class _ProgressTqdm(tqdm.tqdm):
        _last_fraction = -1.0

        def update(self, n=1):
            super().update(n)
            if on_progress is None:
                return
            if total:
                # disk_bytes excludes the current .incomplete file, so add
                # this progress bar's bytes to the completed files on disk.
                frac = min(1.0, (disk_bytes(root, model_id, ct2_subdir) + self.n) / total)
            else:
                frac = min(1.0, self.n / max(self.total or 1, 1))
            if frac < 1.0 and frac - self._last_fraction < 0.005:
                return
            self._last_fraction = frac
            on_progress(frac)
            log(f"download progress {frac*100:.1f}%")

    snapshot_download(
        repo_id=repo_id,
        revision=revision,
        local_dir=str(model_root),
        allow_patterns=allow_patterns,
        tqdm_class=_ProgressTqdm,
        max_workers=8,
    )

    if not is_downloaded(root, model_id, ct2_subdir):
        raise RuntimeError(
            f"download finished but model files are missing/incomplete for '{model_id}'"
        )
    log(f"model '{model_id}' download complete: {disk_bytes(root, model_id, ct2_subdir)/1e6:.0f} MB on disk")
    if on_progress:
        on_progress(1.0)
