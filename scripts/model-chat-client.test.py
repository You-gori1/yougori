"""Offline protocol tests for the generated, dependency-free Python chat client."""
import io
import json
from pathlib import Path
import types
import unittest
from unittest.mock import patch

SOURCE = (Path(__file__).resolve().parents[1] / "cli/src/model_chat.py").read_text(encoding="utf-8")


def client():
    module = types.ModuleType("yougori_test_chat")
    config = json.dumps({"environment": "env-test", "cli": "C:\\Program Files\\Yougori\\yougori.exe"})
    exec(compile(SOURCE.replace("__CONFIG__", config), "yougori_chat.py", "exec"), module.__dict__)
    return module


class ChatClientTests(unittest.TestCase):
    def test_credentials_are_retrieved_from_cli_with_literal_arguments(self):
        app = client()
        result = types.SimpleNamespace(stdout=json.dumps({"ok": True, "result": {"apiUrl": "http://127.0.0.1:8000/v1", "apiKey": "private"}}))
        with patch.dict("os.environ", {"YOUGORI_CLI": "my cli"}), patch.object(app.subprocess, "run", return_value=result) as run:
            self.assertEqual(app.access()["apiKey"], "private")
            self.assertEqual(run.call_args.args[0], ["my cli", "model", "access", "env-test"])
            self.assertNotIn("shell", run.call_args.kwargs)

    def test_streamed_chat_uses_bearer_auth_system_prompt_and_no_proxy(self):
        app = client()
        credentials = {"apiUrl": "http://127.0.0.1:8000/v1", "apiKey": "private", "model": "owner/model"}
        stream = io.BytesIO(b'data: {"choices":[{"delta":{"content":"Hello"}}]}\n\ndata: [DONE]\n\n')
        with patch.object(app, "access", return_value=credentials), patch("builtins.input", side_effect=["Be concise", "Hi", "/exit"]), patch.object(app.urllib.request, "build_opener") as build, patch("sys.stdout", new_callable=io.StringIO) as output:
            build.return_value.open.return_value = stream
            app.main()
            self.assertEqual(build.call_args.args[0].proxies, {})
            request = build.return_value.open.call_args.args[0]
            self.assertEqual(request.get_header("Authorization"), "Bearer private")
            payload = json.loads(request.data)
            self.assertTrue(payload["stream"])
            self.assertEqual(payload["messages"][0], {"role": "system", "content": "Be concise"})
            self.assertIn("Hello", output.getvalue())
            self.assertNotIn("private", output.getvalue())

    def test_cli_failure_is_reported_without_falling_back_to_public_url(self):
        app = client()
        result = types.SimpleNamespace(stdout=json.dumps({"ok": False, "error": "Start the environment"}))
        with patch.object(app.subprocess, "run", return_value=result):
            with self.assertRaisesRegex(RuntimeError, "Start the environment"):
                app.access()


if __name__ == "__main__":
    unittest.main()
