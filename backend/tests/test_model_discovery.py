import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from worker import _supports_ru_en


class DiscoveryLanguageTests(unittest.TestCase):
    def test_multilingual_whisper_accepts_both_languages(self):
        self.assertTrue(_supports_ru_en({"lang_ids": [50259, 50260, 50263]}))

    def test_english_only_and_unrelated_models_are_rejected(self):
        for config in ({"lang_ids": [50259]}, {}, {"lang_ids": None}, []):
            with self.subTest(config=config):
                self.assertFalse(_supports_ru_en(config))
