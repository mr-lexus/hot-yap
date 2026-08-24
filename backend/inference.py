"""Model loading and transcription for the HotYap worker.

The model object lives in the worker process state and is loaded exactly
once (on `load_model`), then reused for every `transcribe` call.
"""

import os
import re
import sys
import time
import traceback
import wave
from pathlib import Path

# The release worker no longer bundles PyAV/FFmpeg: the Rust side resamples the
# recording to 16 kHz and we decode the WAV here with the stdlib `wave` module.
# faster_whisper still does `import av` at import time, so install a no-op
# stand-in when the real PyAV is absent. `faster_whisper.audio.decode_audio` is
# never called because we always pass a numpy array to `WhisperModel.transcribe`.
try:
    import av  # noqa: F401
except Exception:
    import types

    sys.modules.setdefault("av", types.ModuleType("av"))

SUB = "ct2_int8_float16"

CUDA_RUNTIME_DIRNAME = "cuda-runtime"

# Handles returned by os.add_dll_directory(): the directory is removed from
# the DLL search path when the handle is garbage collected, so keep them for
# the lifetime of the process.
_CUDA_RUNTIME_HANDLES = []

# Prompt-conditioning strongly biases Whisper's output style: it teaches the
# decoder to keep embedded English words in latin script instead of writing
# cyrillic transliterations ("Hello World", "git" instead of "хэллоу ворлд").
INITIAL_PROMPT = (
    "Ниже представлен транскрипт русской речи с вкраплениями английских "
    "технических терминов. Английские термины записываются латиницей в "
    "оригинале, например: Python, Git, Docker, useEffect, TypeScript, React, "
    "Hello World."
)

# Safety net: conservative whole-word replacements for the most common cyrillic
# transliterations the model still produces. Only terms whose cyrillic form is
# unambiguous are included. Applied after decoding, case-aware.
TERM_FIXES = [
    ("джит|гит", "git"),
    ("джитхаб|гитхаб", "GitHub"),
    ("питон|пайтон", "Python"),
    ("докер", "Docker"),
    ("юсефект|юзэффект|юзефект|усефект", "useEffect"),
    ("тайпскрипт", "TypeScript"),
    ("джаваскрипт|жаваскрипт", "JavaScript"),
    ("реакт", "React"),
    ("коммит", "commit"),
    ("ребейс", "rebase"),
    ("мердж|мерж", "merge"),
    ("бранч", "branch"),
    ("пуш", "push"),
    ("фронтенд", "frontend"),
    ("бэкенд", "backend"),
    ("хэллоу ворлд|хеллоу ворлд|хелло ворлд", "Hello World"),
]


def log(*a):
    print(*a, file=sys.stderr, flush=True)


def fix_latin_terms(text: str) -> str:
    """Replace cyrillic transliterations of known tech terms with latin form."""
    for pattern, replacement in TERM_FIXES:
        def repl(m):
            word = m.group(0)
            if word[0].isupper():
                return replacement[0].upper() + replacement[1:]
            return replacement

        text = re.sub(rf"\b(?:{pattern})\b", repl, text, flags=re.IGNORECASE)
    return text


def _model_path(model_dir: str, ct2_subdir=None) -> Path:
    p = Path(model_dir)
    return (p / ct2_subdir) if ct2_subdir else p


def _prepare_cuda_runtime(models_root=None):
    """Add a downloaded CUDA runtime directory to the DLL search path.

    The release bundle already ships the DLLs next to the frozen worker, but a
    runtime installed via `download_cuda_runtime` lives under the models
    directory and must be made visible to LoadLibrary before ctranslate2 uses
    it. No-op when the directory is absent (bundled or system runtime).

    Two search-path mechanisms are used because ctranslate2 loads cuBLAS
    lazily on the first encode via plain LoadLibrary:
    - os.add_dll_directory() adds the directory to the DLL search path for
      LoadLibraryEx (its handle MUST be kept alive or the entry is dropped as
      soon as the return value is garbage collected).
    - prepending the directory to PATH covers the default LoadLibrary search
      order (application dir, system dirs, current dir, PATH).

    The directory must be an ABSOLUTE path: AddDllDirectoryW rejects relative
    paths with ERROR_INVALID_PARAMETER, and PATH modifications are ignored by
    LoadLibraryEx in a PyInstaller-frozen process, so neither fallback works
    when the path is relative.
    """
    if sys.platform != "win32" or not models_root:
        return
    runtime_dir = (Path(models_root) / CUDA_RUNTIME_DIRNAME).resolve()
    if not runtime_dir.is_dir():
        return
    try:
        _CUDA_RUNTIME_HANDLES.append(os.add_dll_directory(str(runtime_dir)))
    except OSError as exc:
        log(f"cannot add CUDA runtime dir to DLL search path: {exc}")
    path = os.environ.get("PATH", "")
    if str(runtime_dir) not in path.split(os.pathsep):
        os.environ["PATH"] = str(runtime_dir) + os.pathsep + path


class ModelLoadError(RuntimeError):
    """A model-load failure carrying a user-facing diagnosis.

    `kind` is a stable machine-readable identifier (forwarded to the Rust side
    as `error_kind`); `details` holds structured facts (free memory, model
    size) so the caller can log or display them without re-parsing the text.
    """

    def __init__(self, message: str, kind: str = "model_load_failed", details=None):
        super().__init__(message)
        self.kind = kind
        self.details = details or {}


# Exception texts that mean the HOST could not allocate memory. These appear
# both on pure-CPU loads (MKL) and during CUDA loads (cuBLAS/CT2 stage-in
# buffers are host allocations too), so they take priority over GPU-OOM text.
_SYSTEM_OOM_SIGNATURES = (
    "mkl_malloc",
    "failed to allocate",
    "std::bad_alloc",
    "bad_alloc",
    "cannot allocate memory",
)

# Texts that mean the DEVICE (VRAM) ran out during a CUDA-stage attempt.
_GPU_OOM_SIGNATURES = (
    "out of memory",
    "cuda_error_memory_allocation",
)


def classify_load_error(exc: BaseException, stage: str) -> str:
    """Map a model-load exception to a stable diagnostic kind.

    `stage` is 'cuda' or 'cpu': identical 'out of memory' text means VRAM
    exhaustion on the GPU stage but host-RAM exhaustion once the CPU/MKL path
    is reached.
    """
    text = str(exc).lower()
    if any(sig in text for sig in _SYSTEM_OOM_SIGNATURES):
        return "system_out_of_memory"
    if stage == "cuda" and any(sig in text for sig in _GPU_OOM_SIGNATURES):
        return "gpu_out_of_memory"
    if "nvcuda" in text or "no cuda" in text or "cuda driver" in text:
        return "cuda_unavailable"
    return "model_load_failed"


_KIND_PRIORITY = ("system_out_of_memory", "gpu_out_of_memory", "cuda_unavailable")


def dominant_kind(attempts) -> str:
    """The single most informative kind across every failed load attempt.

    Host-memory failures win over GPU ones on purpose: when MKL cannot
    allocate, the real bottleneck is system RAM regardless of what CUDA said.
    """
    kinds = {classify_load_error(exc, stage) for stage, _, exc in attempts}
    for preferred in _KIND_PRIORITY:
        if preferred in kinds:
            return preferred
    return "model_load_failed"


def system_memory():
    """Best-effort memory facts in bytes, or {} when the platform is unknown.

    On Windows the meaningful pair is `commit_limit` / `commit_free`: a fresh
    allocation needs commit headroom (RAM + page file), not physical free RAM
    — which is exactly why loads fail with gigabytes of RAM still 'free'.
    """
    facts = {}
    if sys.platform == "win32":
        import ctypes

        class _MemoryStatusEx(ctypes.Structure):
            _fields_ = [
                ("dwLength", ctypes.c_ulong),
                ("dwMemoryLoad", ctypes.c_ulong),
                ("ullTotalPhys", ctypes.c_uint64),
                ("ullAvailPhys", ctypes.c_uint64),
                ("ullTotalPageFile", ctypes.c_uint64),
                ("ullAvailPageFile", ctypes.c_uint64),
                ("ullTotalVirtual", ctypes.c_uint64),
                ("ullAvailVirtual", ctypes.c_uint64),
                ("ullAvailExtendedVirtual", ctypes.c_uint64),
            ]

        status = _MemoryStatusEx()
        status.dwLength = ctypes.sizeof(_MemoryStatusEx)
        try:
            if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
                facts["physical_total"] = status.ullTotalPhys
                facts["physical_free"] = status.ullAvailPhys
                facts["commit_limit"] = status.ullTotalPageFile
                facts["commit_free"] = status.ullAvailPageFile
        except Exception as e:  # pragma: no cover - defensive, never fatal
            log(f"cannot query system memory: {e}")
    elif sys.platform == "linux":
        values = {}
        try:
            with open("/proc/meminfo", encoding="ascii") as fh:
                for line in fh:
                    key, _, rest = line.partition(":")
                    values[key.strip()] = int(rest.strip().split()[0]) * 1024
            facts["physical_total"] = values.get("MemTotal")
            facts["physical_free"] = values.get("MemAvailable")
        except (OSError, ValueError):
            return {}
    return {k: v for k, v in facts.items() if v}


def gpu_memory():
    """(vram_total, vram_free) of the first GPU via nvidia-smi, or {}."""
    import subprocess

    flags = subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0
    try:
        out = subprocess.run(
            ["nvidia-smi", "--query-gpu=memory.total,memory.free", "--format=csv,noheader,nounits"],
            capture_output=True,
            text=True,
            timeout=5,
            creationflags=flags,
        )
    except (OSError, subprocess.SubprocessError):
        return {}
    if out.returncode != 0 or not out.stdout.strip():
        return {}
    try:
        total_mib, free_mib = out.stdout.strip().splitlines()[0].split(",")[:2]
        return {
            "vram_total": int(total_mib.strip()) * 1024 * 1024,
            "vram_free": int(free_mib.strip()) * 1024 * 1024,
        }
    except (ValueError, IndexError):
        return {}


def _fmt_gb(num_bytes) -> str:
    return f"{num_bytes / (1024 ** 3):.1f} GB"


def _collect_facts(model_bytes: int) -> dict:
    facts = {"model_bytes": model_bytes}
    facts.update(system_memory())
    facts.update(gpu_memory())
    return facts


def _short_reason(exc: BaseException, limit: int = 140) -> str:
    text = " ".join(str(exc).split())
    return text if len(text) <= limit else text[: limit - 3] + "..."


def _compose_failure_message(attempts, facts: dict) -> str:
    """Turn failed load attempts + memory facts into an actionable diagnosis.

    `attempts` is a list of (stage, compute_type, exception) tuples, in try
    order; `facts` comes from `_collect_facts`.
    """
    kind = dominant_kind(attempts)
    model_bytes = facts.get("model_bytes") or 0

    if kind == "system_out_of_memory":
        headline = "The computer ran out of free memory while loading the model."
    elif kind == "gpu_out_of_memory":
        headline = "The GPU ran out of video memory (VRAM) while loading the model."
    elif kind == "cuda_unavailable":
        headline = "No working CUDA GPU was found for this model."
    else:
        headline = "The model failed to load."

    lines = [headline]
    for stage, c_type, exc in attempts:
        label = "GPU (CUDA)" if stage == "cuda" else "CPU"
        lines.append(f"- {label}, {c_type}: {_short_reason(exc)}.")

    fact_bits = []
    if model_bytes:
        fact_bits.append(f"model size {_fmt_gb(model_bytes)}")
    if facts.get("commit_free") is not None and facts.get("commit_limit"):
        fact_bits.append(
            f"free memory for new programs {_fmt_gb(facts['commit_free'])} "
            f"of {_fmt_gb(facts['commit_limit'])} (RAM + page file)"
        )
    elif facts.get("physical_free") is not None and facts.get("physical_total"):
        fact_bits.append(f"free RAM {_fmt_gb(facts['physical_free'])} of {_fmt_gb(facts['physical_total'])}")
    if attempts and any(stage == "cuda" for stage, _, _ in attempts):
        if facts.get("vram_free") is not None and facts.get("vram_total"):
            fact_bits.append(f"free VRAM {_fmt_gb(facts['vram_free'])} of {_fmt_gb(facts['vram_total'])}")
    if fact_bits:
        lines.append("Measured: " + "; ".join(fact_bits) + ".")

    advice = []
    if kind == "system_out_of_memory":
        advice.append("Close memory-heavy apps (browser tabs, WSL/Docker VMs, games) and press Start model again.")
        if sys.platform == "win32":
            advice.append(
                "Enlarge the virtual memory (page file): Settings > System > About > Advanced system settings "
                "> Performance settings > Advanced > Virtual memory, or set it to automatic, then restart the PC."
            )
        else:
            advice.append("Add swap space (or enlarge the swap file) and try again.")
        if model_bytes and facts.get("commit_free") is not None:
            # Rough sizing rule: the weights need their own footprint (plus
            # activations) as fresh commit charge. If that does not fit, say so.
            needs_smaller_model = model_bytes * 1.5 > facts["commit_free"]
        else:
            needs_smaller_model = model_bytes >= 900_000_000
        if needs_smaller_model:
            advice.append("Choose a smaller model in the Models panel - this one barely fits in free memory.")
        advice.append("Or pick a cloud transcription provider in Settings.")
    else:
        if "gpu_out_of_memory" in {classify_load_error(exc, stage) for stage, _, exc in attempts}:
            advice.append(
                "Free up GPU memory (close GPU-accelerated apps such as Chrome hardware acceleration or games), "
                "or set Device to CPU in Settings, or choose a smaller model."
            )
        if kind == "cuda_unavailable":
            advice.append("Install or update the NVIDIA driver, or set Device to CPU in Settings.")
    if advice:
        lines.append("What you can do:")
        lines.extend(f"- {item}" for item in advice)
    return "\n".join(lines)


def load_model(state: dict, model_dir: str, ct2_subdir=None, models_root=None, device="auto"):
    """Load the model according to the requested device preference ('auto', 'cuda', 'cpu').

    Returns (device, compute_type).
    Raises ModelLoadError with a user-facing diagnosis when everything fails;
    the exception carries `.kind` and `.details` (see ModelLoadError).
    """
    path = _model_path(model_dir, ct2_subdir)
    model_bin = path / "model.bin"
    if not model_bin.exists():
        raise FileNotFoundError(
            f"model files not found at {path}. Download the model first."
        )
    model_bytes = model_bin.stat().st_size

    _prepare_cuda_runtime(models_root)

    import ctranslate2

    t0 = time.monotonic()
    req_device = (device or "auto").lower()
    attempts = []  # (stage, compute_type, exception), in try order

    if req_device in ("auto", "cuda"):
        try:
            n_gpu = ctranslate2.get_cuda_device_count()
        except Exception as e:
            log(f"CUDA probe failed: {e}")
            n_gpu = 0

        if n_gpu > 0:
            log(f"CUDA detected ({n_gpu} device(s)), querying supported compute types...")
            try:
                supported = ctranslate2.get_supported_compute_types("cuda")
            except Exception as e:
                log(f"Failed to query CUDA compute types: {e}")
                supported = set()

            # Prefer float16 for stability and speed on CUDA, then int8_float16, then float32, then int8.
            cuda_candidates = []
            for candidate in ("float16", "int8_float16", "float32", "int8"):
                if not supported or candidate in supported:
                    cuda_candidates.append(candidate)
            if not cuda_candidates:
                cuda_candidates = ["float16", "int8_float16", "float32"]

            for c_type in cuda_candidates:
                try:
                    log(f"Attempting to load model on CUDA ({c_type})...")
                    m = _load_faster_whisper(path, device="cuda", compute_type=c_type)
                    state["model"] = m
                    state["device"] = "cuda"
                    state["compute_type"] = c_type
                    log(f"model loaded on CUDA ({c_type}) in {time.monotonic()-t0:.1f}s")
                    return "cuda", c_type
                except Exception as e:
                    log(f"CUDA model load with compute_type={c_type} failed: {e}")
                    attempts.append(("cuda", c_type, e))

            if req_device == "cuda":
                raise ModelLoadError(
                    _compose_failure_message(attempts, _collect_facts(model_bytes)),
                    dominant_kind(attempts),
                    _collect_facts(model_bytes),
                ) from attempts[-1][2]
            log(f"CUDA model load failed ({attempts[-1][2]}); falling back to CPU (int8)")
        elif req_device == "cuda":
            raise ModelLoadError(
                "No CUDA-capable GPU was detected, but Device is set to CUDA.\n"
                "What you can do:\n"
                "- Install or update the NVIDIA driver, or\n"
                "- Set Device to Auto or CPU in Settings.",
                "cuda_unavailable",
                _collect_facts(model_bytes),
            )

    log("loading model on CPU (int8)...")
    try:
        m = _load_faster_whisper(path, device="cpu", compute_type="int8")
        state["model"] = m
        state["device"] = "cpu"
        state["compute_type"] = "int8"
        log(f"model loaded on CPU in {time.monotonic()-t0:.1f}s")
        return "cpu", "int8"
    except Exception as e:
        attempts.append(("cpu", "int8", e))
        raise ModelLoadError(
            _compose_failure_message(attempts, _collect_facts(model_bytes)),
            dominant_kind(attempts),
            _collect_facts(model_bytes),
        ) from e


def _load_faster_whisper(path: Path, device: str, compute_type: str):
    from faster_whisper import WhisperModel

    return WhisperModel(str(path), device=device, compute_type=compute_type)


def audio_duration(audio_path: str) -> float:
    """Best-effort audio duration in seconds (for timeout sizing)."""
    try:
        with wave.open(audio_path, "rb") as w:
            return w.getnframes() / w.getframerate()
    except Exception:
        return 0.0


def _decode_wav_16k(audio_path: str):
    """Read a 16-bit PCM mono 16 kHz WAV into a float32 numpy array."""
    import numpy as np

    with wave.open(audio_path, "rb") as w:
        channels = w.getnchannels()
        sample_width = w.getsampwidth()
        sample_rate = w.getframerate()
        frames = w.readframes(w.getnframes())
    if sample_width != 2 or channels != 1 or sample_rate != 16000:
        raise RuntimeError(
            f"unsupported WAV format: {channels} ch, "
            f"{sample_width * 8}-bit, {sample_rate} Hz (expected mono 16-bit 16 kHz)"
        )
    return np.frombuffer(frames, dtype="<i2").astype(np.float32) / 32768.0


def transcribe(state: dict, audio_path: str, on_progress=None, on_cancel=None):
    model = state.get("model")
    if model is None:
        raise RuntimeError("model is not loaded; press 'Start model' first")

    t0 = time.monotonic()
    dev = state.get("device", "cpu")
    text_parts = []

    audio = _decode_wav_16k(audio_path)

    try:
        segments, info = model.transcribe(
            audio,
            language="ru",
            task="transcribe",
            # CPU dictation must stay responsive; CUDA keeps the more accurate beam.
            beam_size=1 if dev == "cpu" else 5,
            vad_filter=True,
            vad_parameters={"min_silence_duration_ms": 500},
            condition_on_previous_text=False,
            initial_prompt=INITIAL_PROMPT,
        )
        for seg in segments:
            if on_cancel and on_cancel():
                log("transcription cancelled between segments")
                break
            text_parts.append(seg.text.strip())
            if on_progress and info.duration:
                on_progress(min(0.99, seg.end / info.duration))
    except Exception as e:
        log(f"transcribe execution failed on device={dev}:\n{traceback.format_exc()}")
        raise RuntimeError(f"Transcription failed on {dev}: {e}") from e

    if on_progress:
        on_progress(1.0)
    wall = time.monotonic() - t0

    text = " ".join(text_parts)
    text = re.sub(r"\s+", " ", text).strip()
    text = fix_latin_terms(text)

    audio_s = float(info.duration or 0.0)
    rtf = wall / audio_s if audio_s > 0 else 0.0
    log(f"transcribe: audio={audio_s:.1f}s wall={wall:.2f}s rtf={rtf:.2f}")
    return {
        "text": text,
        "inference_s": round(wall, 2),
        "audio_s": round(audio_s, 2),
        "rtf": round(rtf, 3),
    }
