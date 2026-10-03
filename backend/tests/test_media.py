import math
import tempfile
import unittest
import wave
from pathlib import Path
from unittest.mock import patch

import numpy as np
import av

import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import media


class MediaDecodeTests(unittest.TestCase):
    def test_extracts_audio_from_video_container(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "source.mp4"
            output = Path(temporary) / "output.wav"
            rate = 48_000

            with av.open(str(source), "w") as container:
                audio = container.add_stream("aac", rate=rate)
                audio.layout = "stereo"
                video = container.add_stream("mpeg4", rate=1)
                video.width = 16
                video.height = 16
                video.pix_fmt = "yuv420p"

                video_frame = av.VideoFrame.from_ndarray(
                    np.zeros((16, 16, 3), dtype=np.uint8), format="rgb24"
                )
                for packet in video.encode(video_frame):
                    container.mux(packet)

                total_samples = rate // 2
                for start in range(0, total_samples, 1024):
                    count = min(1024, total_samples - start)
                    timeline = (np.arange(count) + start) / rate
                    signal = (np.sin(2 * math.pi * 440 * timeline) * 0.2).astype(
                        np.float32
                    )
                    audio_frame = av.AudioFrame.from_ndarray(
                        np.vstack((signal, signal)), format="fltp", layout="stereo"
                    )
                    audio_frame.sample_rate = rate
                    for packet in audio.encode(audio_frame):
                        container.mux(packet)

                for packet in audio.encode(None):
                    container.mux(packet)
                for packet in video.encode(None):
                    container.mux(packet)

            info = media.decode_to_wav(str(source), str(output))

            self.assertTrue(info["has_video"])
            self.assertIn("mp4", info["format"])
            with wave.open(str(output), "rb") as wav:
                self.assertEqual(wav.getnchannels(), 1)
                self.assertEqual(wav.getframerate(), 16_000)
                self.assertGreater(wav.getnframes(), 7_000)

    def test_normalizes_stereo_wav_to_mono_16k(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "source.wav"
            output = Path(temporary) / "output.wav"
            rate = 48_000
            duration = 0.4
            samples = np.arange(round(rate * duration))
            signal = (np.sin(2 * math.pi * 440 * samples / rate) * 12_000).astype("<i2")
            stereo = np.column_stack((signal, signal)).reshape(-1)
            with wave.open(str(source), "wb") as wav:
                wav.setnchannels(2)
                wav.setsampwidth(2)
                wav.setframerate(rate)
                wav.writeframes(stereo.tobytes())

            progress = []
            info = media.decode_to_wav(str(source), str(output), progress.append)

            with wave.open(str(output), "rb") as wav:
                self.assertEqual(wav.getnchannels(), 1)
                self.assertEqual(wav.getsampwidth(), 2)
                self.assertEqual(wav.getframerate(), 16_000)
                self.assertAlmostEqual(wav.getnframes() / 16_000, duration, places=2)
            self.assertAlmostEqual(info["duration"], duration, places=2)
            self.assertEqual(progress[-1], 1.0)

    def test_cancellation_removes_partial_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "source.wav"
            output = Path(temporary) / "output.wav"
            with wave.open(str(source), "wb") as wav:
                wav.setnchannels(1)
                wav.setsampwidth(2)
                wav.setframerate(16_000)
                wav.writeframes(np.zeros(16_000, dtype="<i2").tobytes())

            with self.assertRaises(media.MediaCancelled):
                media.decode_to_wav(str(source), str(output), on_cancel=lambda: True)

            self.assertFalse(output.exists())

    def test_decoded_sample_limit_removes_partial_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "source.wav"
            output = Path(temporary) / "output.wav"
            with wave.open(str(source), "wb") as wav:
                wav.setnchannels(1)
                wav.setsampwidth(2)
                wav.setframerate(16_000)
                wav.writeframes(np.zeros(16_000, dtype="<i2").tobytes())

            with patch.object(media, "MAX_OUTPUT_SAMPLES", 1_000):
                with self.assertRaisesRegex(RuntimeError, "longer than 6 hours"):
                    media.decode_to_wav(str(source), str(output))

            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
