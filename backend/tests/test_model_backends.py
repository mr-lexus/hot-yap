import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

BACKEND_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BACKEND_DIR))

import inference
import model_download


def write_large_file(path: Path):
    with path.open("wb") as handle:
        handle.seek(model_download.MIN_MODEL_BYTES)
        handle.write(b"\0")


class ModelBackendTests(unittest.TestCase):
    def test_ctranslate2_requires_large_model_bin(self):
        with tempfile.TemporaryDirectory() as temp:
            model = Path(temp) / "ct2"
            model.mkdir()
            (model / "model.bin").write_bytes(b"incomplete")
            self.assertFalse(model_download.is_downloaded(temp, "ct2"))
            write_large_file(model / "model.bin")
            self.assertFalse(model_download.is_downloaded(temp, "ct2"))
            for name in ("config.json", "tokenizer.json", "vocabulary.json"):
                (model / name).write_text("{}", encoding="utf-8")
            self.assertTrue(model_download.is_downloaded(temp, "ct2"))
            (model / "tokenizer.json").unlink()
            self.assertFalse(model_download.is_downloaded(temp, "ct2"))

    def test_mlx_requires_config_and_weights(self):
        with tempfile.TemporaryDirectory() as temp:
            model = Path(temp) / "mlx"
            model.mkdir()
            write_large_file(model / "weights.safetensors")
            self.assertFalse(model_download.is_downloaded(temp, "mlx", backend="mlx"))
            (model / "config.json").write_text("{}", encoding="utf-8")
            self.assertTrue(model_download.is_downloaded(temp, "mlx", backend="mlx"))

    def test_unknown_backend_is_never_installed(self):
        with tempfile.TemporaryDirectory() as temp:
            self.assertFalse(model_download.is_downloaded(temp, "model", backend="unknown"))

    def test_apple_silicon_detection_is_strict(self):
        self.assertTrue(inference.is_apple_silicon("darwin", "arm64"))
        self.assertFalse(inference.is_apple_silicon("darwin", "x86_64"))
        self.assertFalse(inference.is_apple_silicon("win32", "arm64"))

    def test_mlx_rejects_cpu_device_before_runtime_import(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(
            inference, "is_apple_silicon", return_value=True
        ):
            with self.assertRaises(inference.ModelLoadError) as raised:
                inference.load_model({}, temp, device="cpu", backend="mlx")
            self.assertEqual(raised.exception.kind, "invalid_device")

    def test_unknown_inference_backend_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            with self.assertRaises(inference.ModelLoadError) as raised:
                inference.load_model({}, temp, backend="unknown")
            self.assertEqual(raised.exception.kind, "unsupported_backend")

    def test_release_model_clears_backend_state(self):
        state = {
            "model": object(),
            "model_path": "/tmp/model",
            "backend": "ctranslate2",
            "device": "cpu",
            "compute_type": "int8",
        }
        inference.release_model(state)
        self.assertIsNone(state["model"])
        self.assertIsNone(state["model_path"])
        self.assertIsNone(state["backend"])
        self.assertIsNone(state["device"])
        self.assertIsNone(state["compute_type"])


if __name__ == "__main__":
    unittest.main()
