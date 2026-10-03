"""Safe media decoding for file transcription.

PyAV wheels bundle the FFmpeg libraries used here, so end users do not need
an external ffmpeg executable. Every accepted input is normalized to the same
mono 16 kHz PCM WAV format used by microphone dictation.
"""

from __future__ import annotations

import wave
from pathlib import Path

MAX_MEDIA_DURATION_SECONDS = 6 * 60 * 60
TARGET_SAMPLE_RATE = 16_000
MAX_OUTPUT_SAMPLES = MAX_MEDIA_DURATION_SECONDS * TARGET_SAMPLE_RATE


class MediaCancelled(RuntimeError):
    pass


def decode_to_wav(input_path: str, output_path: str, on_progress=None, on_cancel=None):
    import av

    source = Path(input_path)
    destination = Path(output_path)
    if not source.is_file():
        raise FileNotFoundError("The selected media file no longer exists")

    frames_written = 0
    has_video = False
    format_name = ""
    destination.parent.mkdir(parents=True, exist_ok=True)

    try:
        # Imported files must never make FFmpeg fetch remote resources. The
        # accepted formats only need local file access (plus data/crypto for
        # container-internal payloads).
        with av.open(
            str(source),
            mode="r",
            options={"protocol_whitelist": "file,data,crypto"},
        ) as container:
            if not container.streams.audio:
                raise RuntimeError("The selected file does not contain an audio track")
            audio_stream = container.streams.audio[0]
            has_video = bool(container.streams.video)
            format_name = container.format.name or ""
            duration = _duration_seconds(container, audio_stream)
            if duration and duration > MAX_MEDIA_DURATION_SECONDS:
                raise RuntimeError("Media longer than 6 hours is not supported")

            resampler = av.AudioResampler(
                format="s16",
                layout="mono",
                rate=TARGET_SAMPLE_RATE,
            )
            with wave.open(str(destination), "wb") as output:
                output.setnchannels(1)
                output.setsampwidth(2)
                output.setframerate(TARGET_SAMPLE_RATE)

                for frame in container.decode(audio=0):
                    _check_cancel(on_cancel)
                    for normalized in resampler.resample(frame):
                        frames_written += _write_frame(output, normalized)
                        _check_output_limit(frames_written)
                    if on_progress and duration:
                        position = _frame_position(frame)
                        if position is not None:
                            on_progress(min(0.99, max(0.0, position / duration)))

                for normalized in resampler.resample(None):
                    frames_written += _write_frame(output, normalized)
                    _check_output_limit(frames_written)

        _check_cancel(on_cancel)
        if frames_written == 0:
            raise RuntimeError("The selected file contains no decodable audio")
        if on_progress:
            on_progress(1.0)
        return {
            "duration": round(frames_written / TARGET_SAMPLE_RATE, 3),
            "has_video": has_video,
            "format": format_name,
        }
    except Exception:
        try:
            destination.unlink(missing_ok=True)
        except OSError:
            pass
        raise


def _write_frame(output, frame) -> int:
    import numpy as np

    samples = frame.to_ndarray().reshape(-1).astype("<i2", copy=False)
    output.writeframesraw(samples.tobytes())
    return int(samples.size)


def _duration_seconds(container, stream) -> float:
    if stream.duration is not None and stream.time_base is not None:
        return float(stream.duration * stream.time_base)
    if container.duration is not None:
        import av

        return float(container.duration / av.time_base)
    return 0.0


def _frame_position(frame):
    if frame.pts is None or frame.time_base is None:
        return None
    return float(frame.pts * frame.time_base)


def _check_cancel(on_cancel):
    if on_cancel and on_cancel():
        raise MediaCancelled("Transcription cancelled")


def _check_output_limit(samples_written: int):
    # Some malformed containers omit or lie about their duration. Enforce the
    # limit against decoded PCM as well so a small input cannot fill the disk.
    if samples_written > MAX_OUTPUT_SAMPLES:
        raise RuntimeError("Media longer than 6 hours is not supported")
