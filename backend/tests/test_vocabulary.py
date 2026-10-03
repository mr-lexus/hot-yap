import sys
import types
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import inference


class VocabularyTests(unittest.TestCase):
    def test_local_hints_and_context_preserve_raw_words_for_custom_rules(self):
        calls = []

        class Model:
            def transcribe(self, audio, **options):
                calls.append(options)
                return iter([types.SimpleNamespace(text="докер, да, да.", end=1)]), types.SimpleNamespace(duration=1)

        with patch.object(inference, "_decode_wav_16k", return_value=[]):
            result = inference.transcribe({"model": Model(), "device": "cpu"}, "test.wav",
                                          vocabulary=["useAuth", "Supabase"], context="Мы обсуждали", term_fixes=False)
        self.assertEqual(result["text"], "докер, да, да.")
        self.assertIn("Supabase", calls[0]["hotwords"])
        self.assertIn("Мы обсуждали", calls[0]["initial_prompt"])
        self.assertFalse(calls[0]["condition_on_previous_text"])

    def test_mlx_uses_prompt_hints_without_ct2_only_options(self):
        calls = []
        fake = types.SimpleNamespace(transcribe=lambda *args, **kw: calls.append(kw) or {"text": "докер"})
        with patch.object(inference, "_decode_wav_16k", return_value=[0] * 16000), patch.dict(sys.modules, {"mlx_whisper": fake}):
            result = inference.transcribe({"model": object(), "backend": "mlx", "model_path": "local"}, "test.wav", vocabulary=["ProjectName"], term_fixes=False)
        self.assertEqual(result["text"], "докер")
        self.assertIn("ProjectName", calls[0]["initial_prompt"])
        self.assertNotIn("hotwords", calls[0])

    def test_prompt_inputs_are_bounded(self):
        calls = []
        fake = types.SimpleNamespace(transcribe=lambda *args, **kw: calls.append(kw) or {"text": "ok"})
        with patch.object(inference, "_decode_wav_16k", return_value=[]), patch.dict(sys.modules, {"mlx_whisper": fake}):
            inference.transcribe({"model": object(), "backend": "mlx", "model_path": "local"}, "test.wav", vocabulary=["x" * 1000] * 1000, context="y" * 10000)
        self.assertLess(len(calls[0]["initial_prompt"]), 2200)
