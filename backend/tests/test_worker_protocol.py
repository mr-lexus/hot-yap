import json
import queue
import subprocess
import sys
import threading
import unittest
from collections import defaultdict, deque
from pathlib import Path


class WorkerProtocolTests(unittest.TestCase):
    def setUp(self):
        self.process = subprocess.Popen(
            [sys.executable, str(Path(__file__).with_name("worker_stub.py"))],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, encoding="utf-8",
        )
        self.messages = queue.Queue()
        self.pending = defaultdict(deque)
        self.reader = threading.Thread(target=self.read_messages, daemon=True)
        self.reader.start()
        self.assertEqual(self.receive()["event"], "worker_ready")

    def read_messages(self):
        for line in self.process.stdout:
            self.messages.put(json.loads(line))

    def send(self, value):
        self.process.stdin.write(json.dumps(value, ensure_ascii=False) + "\n")
        self.process.stdin.flush()

    def receive(self):
        try:
            return self.messages.get(timeout=3)
        except queue.Empty:
            self.fail("worker did not respond while transcription was active")

    def response(self, request_id):
        if self.pending[request_id]:
            return self.pending[request_id].popleft()
        while True:
            message = self.receive()
            if message.get("id") == request_id:
                return message
            self.pending[message.get("id")].append(message)

    def tearDown(self):
        self.process.kill()
        self.process.wait(timeout=5)
        self.reader.join(timeout=3)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            stream.close()

    def test_cancel_is_processed_during_inference(self):
        self.send({"id": 1, "command": "transcribe", "audio_path": "wait"})
        self.assertEqual(self.receive()["event"], "test_started")
        self.send({"id": 2, "command": "load_model"})
        self.assertFalse(self.response(2)["ok"])
        self.send({"id": 3, "command": "cancel_transcription", "request_id": 1})
        self.assertEqual(self.response(3)["event"], "cancel_ack")
        self.assertEqual(self.response(1)["error"], "Transcription cancelled")
        self.send({"id": 4, "command": "transcribe", "audio_path": "новая запись.wav"})
        self.assertEqual(self.response(4)["text"], "новая запись.wav")

    def test_cancel_can_arrive_before_transcription(self):
        self.send({"id": 1, "command": "cancel_transcription", "request_id": 2})
        self.assertEqual(self.response(1)["event"], "cancel_ack")
        self.send({"id": 2, "command": "transcribe", "audio_path": "wait"})
        self.assertEqual(self.response(2)["error"], "Transcription cancelled")

    def test_non_object_request_does_not_kill_worker(self):
        self.send([])
        self.send({"id": 1, "command": "status"})
        self.assertEqual(self.response(1)["event"], "status")

    def test_live_chunks_forward_context_and_finish_before_next_chunk(self):
        for rid in (1, 2, 3):
            self.send({"id": rid, "command": "transcribe", "audio_path": f"chunk-{rid}",
                       "vocabulary": ["Supabase", "useAuth"], "context": "Предыдущая фраза,", "term_fixes": False})
            result = self.response(rid)
            self.assertTrue(result["ok"])
            self.assertEqual(result["text"], f"chunk-{rid}")
            self.assertEqual(result["test_options"]["vocabulary"], ["Supabase", "useAuth"])
            self.assertFalse(result["test_options"]["term_fixes"])

    def test_media_preparation_uses_the_background_protocol(self):
        self.send({
            "id": 1,
            "command": "prepare_media",
            "input_path": "clip.mp4",
            "output_path": "clip.wav",
        })
        response = self.response(1)
        self.assertTrue(response["ok"])
        self.assertEqual(response["event"], "media_prepared")
        self.assertTrue(response["has_video"])


if __name__ == "__main__":
    unittest.main()
