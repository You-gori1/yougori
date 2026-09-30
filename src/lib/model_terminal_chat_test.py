import importlib.util
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("chat", Path(__file__).with_name("model-terminal-chat.py"))
chat = importlib.util.module_from_spec(spec)
spec.loader.exec_module(chat)


class ModelTerminalChatTests(unittest.TestCase):
    def test_system_prompt_is_saved_and_sent_before_user_messages(self):
        with tempfile.TemporaryDirectory() as directory:
            settings = Path(directory) / "settings.json"
            def request(token, path, body=None):
                if path == "/health":
                    return {"status": "ready"}
                self.assertEqual(body["messages"], [{"role": "system", "content": "Be concise"}, {"role": "user", "content": "Hello"}])
                return {"choices": [{"message": {"content": "Hi"}}]}
            with patch("builtins.input", side_effect=["Be concise", "Hello", "/exit"]), patch.object(chat, "request", side_effect=request), patch("builtins.print"):
                chat.chat("secret", "owner/model", settings)
            self.assertEqual(chat.load_prompt(settings), "Be concise")

    def test_changing_system_prompt_resets_history(self):
        with tempfile.TemporaryDirectory() as directory:
            settings = Path(directory) / "settings.json"
            calls = []
            def request(token, path, body=None):
                if path == "/health":
                    return {"status": "ready"}
                calls.append(body["messages"])
                return {"choices": [{"message": {"content": "Reply"}}]}
            with patch("builtins.input", side_effect=["", "First", "/system", "Speak French", "Second", "/exit"]), patch.object(chat, "request", side_effect=request), patch("builtins.print"):
                chat.chat("secret", "owner/model", settings)
            self.assertEqual(calls[-1], [{"role": "system", "content": "Speak French"}, {"role": "user", "content": "Second"}])
            self.assertEqual(chat.load_prompt(settings), "Speak French")

    def test_error_does_not_add_failed_turn_to_history_or_reveal_key(self):
        with tempfile.TemporaryDirectory() as directory:
            calls = []
            def request(token, path, body=None):
                if path == "/health":
                    return {"status": "ready"}
                calls.append(body["messages"])
                if len(calls) == 1:
                    raise ValueError("secret failed")
                return {"choices": [{"message": {"content": "OK"}}]}
            with patch("builtins.input", side_effect=["/clear", "Fail", "Retry", "/exit"]), patch.object(chat, "request", side_effect=request), patch("builtins.print") as output:
                chat.chat("secret", "owner/model", Path(directory) / "settings.json")
            self.assertEqual(calls[-1], [{"role": "user", "content": "Retry"}])
            self.assertNotIn("secret", str(output.call_args_list))

    def test_model_output_cannot_emit_terminal_controls(self):
        self.assertNotIn("\x1b", chat.terminal_text("\x1b]52;c;clipboard\x07"))
        self.assertNotIn("\x07", chat.terminal_text("\x1b]52;c;clipboard\x07"))


    @unittest.skipUnless(__import__("os").name == "posix", "Requires a Linux PTY")
    def test_bootstrap_runs_in_real_shell_and_returns_to_shell(self):
        import base64
        import os
        import pty
        import select
        import subprocess
        import sys
        import time
        with tempfile.TemporaryDirectory() as directory:
            os.symlink(sys.executable, Path(directory) / "python")
            master, slave = pty.openpty()
            environment = dict(os.environ, HOME=directory, PATH=directory + ":" + os.environ["PATH"], YOUGORI_MODEL="owner/model", YOUGORI_MODEL_TOKEN="test-token", TERM="xterm")
            process = subprocess.Popen(["/bin/bash", "--noprofile", "--norc", "-i"], stdin=slave, stdout=slave, stderr=slave, env=environment, start_new_session=True)
            os.close(slave)
            def wait_for(marker):
                output = b""
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    if select.select([master], [], [], 0.1)[0]:
                        output += os.read(master, 65536)
                        if marker in output:
                            return
                self.fail("Terminal did not reach " + repr(marker) + ": " + repr(output[-1000:]))
            try:
                encoded = base64.b64encode(Path(__file__).with_name("model-terminal-chat.py").read_bytes()).decode()
                lines = "\n".join(encoded[i:i + 512] for i in range(0, len(encoded), 512))
                command = "python -c 'import base64;exec(compile(base64.b64decode(\"\"\"\n" + lines + "\n\"\"\"),\"yougori-model-chat\",\"exec\"))'\r"
                for start in range(0, len(command), 2048):
                    chunk = command[start:start + 2048].encode()
                    while chunk:
                        chunk = chunk[os.write(master, chunk):]
                wait_for(b"System prompt (Enter to keep")
                os.write(master, b"Be concise\n")
                wait_for(b"You > ")
                os.write(master, b"/exit\n")
                os.write(master, b"printf 'SHELL_%s\\n' READY\n")
                wait_for(b"SHELL_READY")
                self.assertEqual(chat.load_prompt(Path(directory) / ".config/yougori/model-chat.json"), "Be concise")
            finally:
                process.terminate()
                try:
                    process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                os.close(master)


if __name__ == "__main__":
    unittest.main()
