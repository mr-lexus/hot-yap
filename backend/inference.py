"""Model loading and transcription for the HotYap worker.

The model object lives in the worker process state and is loaded exactly
once (on `load_model`), then reused for every `transcribe` call.
"""

import os
import re
import sys
import time
from pathlib import Path

SUB = "ct2_int8_float16"

CUDA_RUNTIME_DIRNAME = "cuda-runtime"

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
    """
    if sys.platform != "win32" or not models_root:
        return
    runtime_dir = Path(models_root) / CUDA_RUNTIME_DIRNAME
    if runtime_dir.is_dir():
        os.add_dll_directory(str(runtime_dir))


def load_model(state: dict, model_dir: str, ct2_subdir=None, models_root=None):
    """Load the model. Tries CUDA first, falls back to CPU on any failure.

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
    try:
        n_gpu = ctranslate2.get_cuda_device_count()
    except Exception as e:
        log(f"CUDA probe failed: {e}")
        n_gpu = 0

    if n_gpu > 0:
        log(f"CUDA detected ({n_gpu} device(s)), loading model on GPU (int8_float16)...")
        try:
            m = _load_faster_whisper(path, device="cuda", compute_type="int8_float16")
            state["model"] = m
            state["device"] = "cuda"
            state["compute_type"] = "int8_float16"
            log(f"model loaded on CUDA in {time.monotonic()-t0:.1f}s")
            return "cuda", "int8_float16"
        except Exception as e:
            log(f"CUDA model load failed ({e}); falling back to CPU (int8)")

    log("loading model on CPU (int8)...")
    try:
        m = _load_faster_whisper(path, device="cpu", compute_type="int8")
        state["model"] = m
        state["device"] = "cpu"
        state["compute_type"] = "int8"
        log(f"model loaded on CPU in {time.monotonic()-t0:.1f}s")
        return "cpu", "int8"
    except Exception as e:
        raise RuntimeError(f"model load failed on both CUDA and CPU: {e}") from e


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


def transcribe(state: dict, audio_path: str, on_progress=None):
    model = state.get("model")
    if model is None:
        raise RuntimeError("model is not loaded; press 'Start model' first")

    t0 = time.monotonic()
    segments, info = model.transcribe(
        audio_path,
        language="ru",
        task="transcribe",
        # CPU dictation must stay responsive; CUDA keeps the more accurate beam.
        beam_size=1 if state.get("device") == "cpu" else 5,
        vad_filter=True,
        vad_parameters={"min_silence_duration_ms": 500},
        condition_on_previous_text=False,
        initial_prompt=INITIAL_PROMPT,
    )
    # The segment generator decodes progressively, so the running end-time of
    # the last yielded segment is a good real-progress signal.
    text_parts = []
    for seg in segments:
        text_parts.append(seg.text.strip())
        if on_progress and info.duration:
            on_progress(min(0.99, seg.end / info.duration))
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
