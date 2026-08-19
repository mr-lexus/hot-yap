"""Model loading and transcription for the HotYap worker.

The model object lives in the worker process state and is loaded exactly
once (on `load_model`), then reused for every `transcribe` call.
"""

import os
import re
import sys
import time
import traceback
from pathlib import Path

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


def load_model(state: dict, model_dir: str, ct2_subdir=None, models_root=None, device="auto"):
    """Load the model according to the requested device preference ('auto', 'cuda', 'cpu').

    Returns (device, compute_type).
    """
    path = _model_path(model_dir, ct2_subdir)
    model_bin = path / "model.bin"
    if not model_bin.exists():
        raise FileNotFoundError(
            f"model files not found at {path}. Download the model first."
        )

    _prepare_cuda_runtime(models_root)

    import ctranslate2

    t0 = time.monotonic()
    req_device = (device or "auto").lower()

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

            cuda_error = None
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
                    cuda_error = e

            if req_device == "cuda":
                raise RuntimeError(f"CUDA model load failed: {cuda_error}") from cuda_error
            log(f"CUDA model load failed ({cuda_error}); falling back to CPU (int8)")
        elif req_device == "cuda":
            raise RuntimeError("CUDA device was requested, but no CUDA GPU was detected.")

    log("loading model on CPU (int8)...")
    try:
        m = _load_faster_whisper(path, device="cpu", compute_type="int8")
        state["model"] = m
        state["device"] = "cpu"
        state["compute_type"] = "int8"
        log(f"model loaded on CPU in {time.monotonic()-t0:.1f}s")
        return "cpu", "int8"
    except Exception as e:
        raise RuntimeError(f"model load failed: {e}") from e


def _load_faster_whisper(path: Path, device: str, compute_type: str):
    from faster_whisper import WhisperModel

    return WhisperModel(str(path), device=device, compute_type=compute_type)


def audio_duration(audio_path: str) -> float:
    """Best-effort audio duration in seconds (for timeout sizing)."""
    try:
        import wave

        with wave.open(audio_path, "rb") as w:
            return w.getnframes() / w.getframerate()
    except Exception:
        try:
            from faster_whisper.audio import decode_audio

            return float(len(decode_audio(audio_path)) / 16000.0)
        except Exception:
            return 0.0


def transcribe(state: dict, audio_path: str, on_progress=None, on_cancel=None):
    model = state.get("model")
    if model is None:
        raise RuntimeError("model is not loaded; press 'Start model' first")

    t0 = time.monotonic()
    dev = state.get("device", "cpu")
    text_parts = []

    try:
        segments, info = model.transcribe(
            audio_path,
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
