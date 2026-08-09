"""Persistent Python worker for HotYap.

Protocol: newline-delimited JSON on stdin/stdout. stdout carries ONLY
protocol messages; all diagnostics go to stderr.

Rust -> worker:
    {"id":1,"command":"status","model_dir":"...","catalog":[...]}
    {"id":2,"command":"download_model","model_dir":"...","model_id":"...","repo_id":"...","allow_patterns":["..."],"ct2_subdir":"..."}
    {"id":3,"command":"delete_model","model_dir":"...","model_id":"..."}
    {"id":4,"command":"load_model","model_dir":"...","ct2_subdir":"..."}
    {"id":5,"command":"transcribe","audio_path":"..."}
    {"id":6,"command":"shutdown"}

worker -> Rust:
    {"event":"worker_ready","version":"..."}
    {"id":1,"ok":true,"event":"status","models":[...],"engine_status":"loaded","device":"...","compute_type":"..."}
    {"id":2,"ok":true,"event":"download_progress","fraction":0.42}
    {"id":2,"ok":true,"event":"model_downloaded"}
    {"id":3,"ok":true,"event":"model_deleted"}
    {"id":4,"ok":true,"event":"model_loaded","device":"cpu","compute_type":"int8"}
    {"id":5,"ok":true,"event":"transcribed","text":"...","inference_s":1.4,"audio_s":8.2,"rtf":0.17}
    {"id":5,"ok":true,"event":"transcribe_progress","elapsed":1.2,"fraction":0.3}
    {"id":6,"ok":true,"event":"shutdown_ack"}
    {"id":4,"ok":false,"error":"human readable error"}

Commands are executed sequentially (single stdin loop) so the model is
never used concurrently.
"""

import json
import os
import sys
import threading
import time
import traceback

VERSION = "0.1.0"

state = {
    "model": None,
    "device": None,
    "compute_type": None,
    "busy": False,
}

HANDLERS = {}


def log(*a):
    print(*a, file=sys.stderr, flush=True)


def reply(req_id, payload):
    msg = dict(payload)
    msg["id"] = req_id
    print(json.dumps(msg, ensure_ascii=False), flush=True)


def handle(name):
    def deco(fn):
        HANDLERS[name] = fn
        return fn

    return deco


def _emit_progress(req_id, fraction):
    reply(req_id, {"ok": True, "event": "download_progress", "fraction": round(fraction, 4)})


def _emit_transcribe_progress(req_id, elapsed, fraction=None):
    payload = {"ok": True, "event": "transcribe_progress", "elapsed": round(elapsed, 1)}
    if fraction is not None:
        payload["fraction"] = round(min(1.0, fraction), 4)
    reply(req_id, payload)


def _catalog_slug(repo_id, variant):
    value = f"{repo_id}-{variant or 'root'}".lower()
    value = "".join(char if char.isalnum() else "-" for char in value)
    return "hf-" + "-".join(part for part in value.split("-") if part)[:70]


def _catalog_family(repo_id):
    value = repo_id.lower()
    if "codeswitch" in value or "code-switch" in value:
        return "Code Switch"
    if "russian" in value or "-rus" in value or value.endswith("-ru"):
        return "Russian First"
    return "RuEn"


def _discover_repo_variants(api, repo_id):
    """Return only repositories that expose a faster-whisper-compatible CT2 layout."""
    root = list(api.list_repo_tree(repo_id, recursive=False))
    paths = {getattr(item, "path", "") for item in root}
    variants = []

    if "model.bin" in paths and "config.json" in paths:
        variants.append((None, ["*.bin", "*.json", "*.txt"]))

    known_subdirs = {
        "ct2_int8_float16",
        "ct2-int16",
        "ct2_int8",
        "ct2_float16",
        "ct2_float32",
    }
    for path in sorted(paths):
        if path in known_subdirs:
            variants.append((path, [f"{path}/*"]))

    result = []
    for subdir, patterns in variants:
        listing = root if subdir is None else list(
            api.list_repo_tree(repo_id, path_in_repo=subdir, recursive=True)
        )
        files = [item for item in listing if getattr(item, "path", "").endswith("model.bin")]
        if not files:
            continue
        size = sum(getattr(item, "size", 0) or 0 for item in listing)
        result.append({"subdir": subdir, "patterns": patterns, "size": size})
    return result


@handle("discover_models")
def cmd_discover_models(req):
    """Discover candidates, but never download or auto-install them."""
    from huggingface_hub import HfApi

    api = HfApi()
    queries = req.get("queries") or ["whisper russian", "whisper codeswitch", "faster-whisper"]
    per_query = min(max(int(req.get("limit", 24)), 1), 40)
    candidates = {}
    for query in queries:
        try:
            models = api.list_models(
                search=query,
                sort="downloads",
                limit=per_query,
            )
            for info in models:
                repo_id = getattr(info, "id", "")
                lowered = repo_id.lower()
                if "whisper" not in lowered:
                    continue
                if "codeswitch" in lowered and not any(token in lowered for token in ("russian", "-ru", "_ru")):
                    continue
                candidates[repo_id] = info
                if len(candidates) >= 40:
                    break
        except Exception as exc:
            log(f"model discovery query '{query}' failed: {exc}")
        if len(candidates) >= 40:
            break

    discovered = []
    for repo_id, info in candidates.items():
        try:
            variants = _discover_repo_variants(api, repo_id)
        except Exception as exc:
            log(f"skipping incompatible repository {repo_id}: {exc}")
            continue
        family = _catalog_family(repo_id)
        for variant in variants:
            subdir = variant["subdir"]
            suffix = subdir or "int8"
            name = repo_id.rsplit("/", 1)[-1].replace("-", " ")
            format_label = {
                "ct2_int8_float16": "CTranslate2 · Int8 / Float16",
                "ct2-int16": "CTranslate2 · Int16",
                "ct2_int8": "CTranslate2 · Int8",
                "ct2_float16": "CTranslate2 · Float16",
                "ct2_float32": "CTranslate2 · Float32",
                None: "CTranslate2 · Int8",
            }.get(subdir, "CTranslate2")
            discovered.append(
                {
                    "id": _catalog_slug(repo_id, suffix),
                    "name": f"{family} · {name} · {suffix}",
                    "description": f"Compatible CTranslate2 model discovered from {repo_id}.",
                    "family": family,
                    "format": format_label,
                    "size_mb": max(1, round(variant["size"] / 1_000_000)),
                    "repo_id": repo_id,
                    "ct2_subdir": subdir,
                    "allow_patterns": variant["patterns"],
                    "source_url": f"https://huggingface.co/{repo_id}",
                    "revision": getattr(info, "sha", None),
                    "updated_at": str(getattr(info, "lastModified", "")) or None,
                    "downloads": getattr(info, "downloads", None),
                    "tags": ["RU", "EN", family, suffix],
                    "downloaded": False,
                    "loaded": False,
                    "tier": "heavy" if variant["size"] >= 1_000_000_000 else "medium" if variant["size"] >= 300_000_000 else "light",
                }
            )
            if len(discovered) >= 60:
                break
        if len(discovered) >= 60:
            break
    return {"event": "model_catalog", "models": discovered}


@handle("status")
def cmd_status(req):
    from model_download import disk_bytes, model_status

    catalog = req.get("catalog") or []
    models = []
    for spec in catalog:
        mid = spec.get("id")
        sub = spec.get("ct2_subdir")
        models.append(
            {
                "id": mid,
                "downloaded": model_status(req.get("model_dir"), mid, sub) == "downloaded",
                "size_bytes": disk_bytes(req.get("model_dir"), mid, sub),
            }
        )
    return {
        "event": "status",
        "models": models,
        "engine_status": "loaded" if state["model"] is not None else "stopped",
        "device": state["device"],
        "compute_type": state["compute_type"],
    }


@handle("download_model")
def cmd_download(req):
    from model_download import download_model

    req_id = req.get("id")
    mid = req.get("model_id")
    download_model(
        req.get("model_dir"),
        mid,
        req.get("repo_id"),
        req.get("allow_patterns"),
        req.get("ct2_subdir"),
        req.get("revision"),
        on_progress=lambda f: _emit_progress(req_id, f),
    )
    return {"event": "model_downloaded"}


@handle("delete_model")
def cmd_delete(req):
    from model_download import delete_model

    delete_model(req.get("model_dir"), req.get("model_id"))
    return {"event": "model_deleted"}


@handle("load_model")
def cmd_load(req):
    import inference

    device, compute_type = inference.load_model(
        state, req.get("model_dir"), req.get("ct2_subdir")
    )
    return {"event": "model_loaded", "device": device, "compute_type": compute_type}


@handle("transcribe")
def cmd_transcribe(req):
    import inference

    if state.get("busy"):
        return {
            "ok": False,
            "error": (
                "another transcription is still running; "
                "wait for it to finish or restart the engine"
            ),
        }
    audio_path = req.get("audio_path")
    req_id = req.get("id")
    t0 = time.monotonic()

    # Long-running inference must not block the protocol loop: run it in a
    # daemon thread and pump progress events (and the timeout watchdog) from
    # the main loop while it works.
    result_box = {}

    def _run():
        try:
            def _on_progress(frac):
                result_box["fraction"] = frac

            result_box["result"] = inference.transcribe(state, audio_path, on_progress=_on_progress)
        except Exception as e:
            log(f"transcribe failed:\n{traceback.format_exc()}")
            result_box["error"] = f"{type(e).__name__}: {e}"

    state["busy"] = True
    thread = threading.Thread(target=_run, daemon=True)
    thread.start()

    try:
        audio_s = inference.audio_duration(audio_path)
    except Exception:
        audio_s = 0.0
    # Generous but finite: this box's CPU can run at RTF ~15-20, so a long
    # dictation legitimately takes minutes. Only a real hang should time out.
    timeout_s = max(300.0, audio_s * 45.0 + 240.0)
    deadline = t0 + timeout_s
    last_emit = t0

    while thread.is_alive():
        time.sleep(0.5)
        now = time.monotonic()
        if now - last_emit >= 0.75:
            frac = result_box.get("fraction")
            _emit_transcribe_progress(req_id, now - t0, frac)
            last_emit = now
        if now > deadline:
            log(f"transcription timed out after {timeout_s:.0f}s; model may be stuck")
            # Leave busy=True: the stuck thread still owns the model. The only
            # clean recovery is restart_worker (kills the process).
            return {
                "ok": False,
                "error": (
                    f"transcription timed out after {int(timeout_s)}s — "
                    "the model may be stuck. Cancel and restart the engine."
                ),
            }

    state["busy"] = False
    if "result" in result_box:
        return {"event": "transcribed", **result_box["result"]}
    return {"ok": False, "error": result_box.get("error", "transcription failed")}


def main():
    log(f"hotyap worker {VERSION} starting (pid={__import__('os').getpid()})")
    print(json.dumps({"event": "worker_ready", "version": VERSION}), flush=True)
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except Exception as e:
            log(f"ignoring malformed request: {e}")
            continue
        rid = req.get("id")
        cmd = req.get("command")
        if cmd == "shutdown":
            reply(rid, {"ok": True, "event": "shutdown_ack"})
            log("shutdown requested, exiting")
            break
        handler = HANDLERS.get(cmd)
        if handler is None:
            reply(rid, {"ok": False, "error": f"unknown command: {cmd}"})
            continue
        try:
            result = handler(req) or {}
            reply(rid, {"ok": True, **result})
        except Exception as e:
            log(f"command '{cmd}' failed:\n{traceback.format_exc()}")
            reply(rid, {"ok": False, "error": f"{type(e).__name__}: {e}"})
    log("worker exiting")


if __name__ == "__main__":
    main()
