"""Protocol test worker; no model, network, or GPU dependencies."""
import sys
import types
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import worker


def transcribe(state, audio_path, on_progress, on_cancel, **options):
    worker.reply(None, {"event": "test_started"})
    if audio_path == "wait":
        if not state["cancel_event"].wait(10):
            raise RuntimeError("test transcription was not cancelled")
    return {"text": audio_path, "test_options": options}


sys.modules["inference"] = types.SimpleNamespace(
    transcribe=transcribe, audio_duration=lambda _: 1.0,
)
sys.modules["media"] = types.SimpleNamespace(
    decode_to_wav=lambda input_path, output_path, on_progress, on_cancel: {
        "duration": 1.0,
        "has_video": input_path.endswith(".mp4"),
        "format": "test",
    }
)
worker.main()
