import importlib.util
import http.client
import json
import os
from pathlib import Path
import threading
import unittest
from contextlib import contextmanager, ExitStack
from unittest.mock import Mock, patch
from types import SimpleNamespace
from urllib.request import Request, urlopen
from urllib.error import HTTPError

os.environ["YOUGORI_MODEL"] = "example/test-model"
os.environ["YOUGORI_MODEL_TOKEN"] = "a" * 64
spec = importlib.util.spec_from_file_location("model_server", Path(__file__).parents[1] / "src-tauri/src/model_server.py")
server = importlib.util.module_from_spec(spec)
spec.loader.exec_module(server)

class ChatConfig:
    model_type = "llama"
    max_position_embeddings = 2048


@contextmanager
def model_startup(count=1, files=(), config=None, versions=None):
    config = config or ChatConfig()
    network = Mock(config=config)
    network.eval.return_value = network
    models = Mock()
    models._model_mapping = {ChatConfig: object}
    models.from_pretrained.return_value = network
    transformers = SimpleNamespace(AutoConfig=Mock(), AutoModelForCausalLM=models, AutoTokenizer=Mock())
    transformers.AutoConfig.from_pretrained.return_value = config
    hub = SimpleNamespace(HfApi=Mock())
    hub.HfApi.return_value.model_info.return_value = SimpleNamespace(
        sha="a" * 40, siblings=[SimpleNamespace(rfilename=name) for name in files])
    torch = SimpleNamespace(float16="float16", cuda=SimpleNamespace(
        is_available=lambda: True, device_count=lambda: count, get_device_name=lambda index: "Test GPU"))
    installed = {"transformers": "5.18.0", "accelerate": "1.15.0", "huggingface-hub": "1.33.0", **(versions or {})}
    with ExitStack() as stack:
        stack.enter_context(patch.dict("sys.modules", torch=torch, transformers=transformers, huggingface_hub=hub))
        stack.enter_context(patch.object(server.importlib.metadata, "version", side_effect=installed.__getitem__))
        stack.enter_context(patch.object(server.threading, "Thread"))
        install = stack.enter_context(patch.object(server.subprocess, "run"))
        stack.enter_context(patch.dict(os.environ, YOUGORI_INSTALL_TORCH="0"))
        stack.enter_context(patch.dict(server.STATE, status="installing", error=None))
        for name in ("NETWORK", "TOKENIZER", "TORCH"):
            stack.enter_context(patch.object(server, name, None))
        yield SimpleNamespace(transformers=transformers, hub=hub, install=install, models=models, config=config)


class ModelServerTests(unittest.TestCase):
    def test_malformed_saved_usage_cannot_break_request_accounting(self):
        import tempfile
        with tempfile.TemporaryDirectory() as folder, \
             patch.object(server, "USAGE_PATH", os.path.join(folder, "usage.json")):
            for change in ({"totals": {"requests": "broken"}},
                           {"sources": {"api": None}},
                           {"hours": {str(int(server.time.time() // 3600)): []}}):
                with self.subTest(change=change):
                    value = server.empty_usage()
                    value.update(change)
                    Path(server.USAGE_PATH).write_text(json.dumps(value))
                    with patch.object(server, "USAGE", server.load_usage()):
                        server.record_usage("api", "ok", 2, 1)
                        self.assertEqual(server.load_usage()["totals"]["requests"], 1)

    def test_slow_usage_reader_does_not_block_recording_model_requests(self):
        import tempfile
        replying, release = threading.Event(), threading.Event()

        class SlowReader(server.Handler):
            def reply(self, status, value):
                replying.set()
                release.wait(3)
                super().reply(status, value)

        with tempfile.TemporaryDirectory() as folder, \
             patch.object(server, "USAGE_PATH", os.path.join(folder, "usage.json")), \
             patch.object(server, "USAGE", server.empty_usage()):
            listener = server.Server(("127.0.0.1", 0), SlowReader)
            worker = threading.Thread(target=listener.serve_forever, daemon=True)
            worker.start()
            client = http.client.HTTPConnection("127.0.0.1", listener.server_port, timeout=4)
            recorded = threading.Event()
            recorder = None
            try:
                client.request("GET", "/v1/usage", headers={"Authorization": "Bearer " + "a" * 64})
                self.assertTrue(replying.wait(2))
                def record():
                    server.record_usage("api", "ok", 2, 1)
                    recorded.set()
                recorder = threading.Thread(target=record)
                recorder.start()
                self.assertTrue(recorded.wait(1), "A usage response held the lock while sending to its client")
                release.set()
                response = client.getresponse()
                self.assertEqual(response.status, 200)
                self.assertEqual(json.loads(response.read())["totals"]["requests"], 0)
                self.assertEqual(server.load_usage()["totals"]["requests"], 1)
            finally:
                release.set()
                if recorder:
                    recorder.join(4)
                client.close()
                listener.shutdown()
                listener.server_close()
                worker.join()

    def test_non_ascii_api_key_is_rejected_with_an_http_response(self):
        listener = server.Server(("127.0.0.1", 0), server.Handler)
        worker = threading.Thread(target=listener.serve_forever, daemon=True)
        worker.start()
        client = http.client.HTTPConnection("127.0.0.1", listener.server_port, timeout=2)
        try:
            client.request("GET", "/health", headers={"Authorization": "Bearer \u00e9"})
            response = client.getresponse()
            self.assertEqual(response.status, 401)
            self.assertEqual(json.loads(response.read())["error"]["message"], "A model API token is required")
        finally:
            client.close()
            listener.shutdown()
            listener.server_close()
            worker.join()

    def test_model_loading_uses_all_visible_gpus_when_more_than_one_is_attached(self):
        for count in [1, 2, 8]:
            with self.subTest(count=count), model_startup(count=count) as fixture:
                server.load_model()
                self.assertEqual(server.STATE["status"], "ready", server.STATE["error"])
                self.assertEqual(server.STATE["gpuCount"], count)
                fixture.install.assert_not_called()
                options = fixture.models.from_pretrained.call_args.kwargs
                self.assertEqual(options["device_map"], "balanced" if count > 1 else {"": 0})
                self.assertEqual(options["dtype"], "auto")
                self.assertFalse(options["trust_remote_code"])
                self.assertTrue(options["use_safetensors"])

    def test_model_start_upgrades_dependencies_that_cannot_recognize_qwen3_5(self):
        with model_startup(versions={"transformers": "4.57.2", "accelerate": "1.11.0"}) as fixture:
            server.load_model()
            self.assertEqual(server.STATE["status"], "ready", server.STATE["error"])
            fixture.install.assert_called_once()
            installed = fixture.install.call_args.args[0]
            self.assertIn("transformers==5.18.0", installed)
            self.assertIn("accelerate==1.15.0", installed)
            self.assertNotIn("torch==2.8.0", installed, "Do not replace the existing CUDA PyTorch")

    def test_dependency_failure_stops_before_model_downloads(self):
        with model_startup(versions={"transformers": "4.57.2", "accelerate": "1.11.0"}) as fixture:
            fixture.install.side_effect = server.subprocess.CalledProcessError(1, "pip")
            server.load_model()
            self.assertEqual(server.STATE["status"], "error")
            fixture.hub.HfApi.assert_not_called()
            fixture.models.from_pretrained.assert_not_called()

    def test_custom_decision_head_is_not_silently_ignored_by_the_chat_runner(self):
        with model_startup(files=("config.json", "model.safetensors", "joint_head_config.json", "joint_head.safetensors")) as fixture, \
             patch.object(server, "MODEL", "Cloudflare/clef"):
            server.load_model()
            self.assertEqual(server.STATE["status"], "error")
            self.assertIn("decision", server.STATE["error"])
            self.assertIn("Cloudflare/clef", server.STATE["error"])
            fixture.transformers.AutoConfig.from_pretrained.assert_not_called()
            fixture.transformers.AutoTokenizer.from_pretrained.assert_not_called()
            fixture.models.from_pretrained.assert_not_called()

    def test_non_chat_architecture_is_rejected_before_tokenizer_and_weights(self):
        with model_startup(config=SimpleNamespace(model_type="bert")) as fixture:
            server.load_model()
            self.assertEqual(server.STATE["status"], "error")
            self.assertIn("bert", server.STATE["error"])
            fixture.transformers.AutoTokenizer.from_pretrained.assert_not_called()
            fixture.models.from_pretrained.assert_not_called()

    def test_config_tokenizer_and_weights_use_the_same_checkpoint_revision(self):
        with model_startup() as fixture:
            server.load_model()
            self.assertEqual(server.STATE["status"], "ready", server.STATE["error"])
            fixture.hub.HfApi.return_value.model_info.assert_called_once_with(server.MODEL, timeout=30)
            for loader in (fixture.transformers.AutoConfig, fixture.transformers.AutoTokenizer, fixture.models):
                options = loader.from_pretrained.call_args.kwargs
                self.assertEqual(options["revision"], "a" * 40)
                self.assertFalse(options["trust_remote_code"])
            self.assertIs(fixture.models.from_pretrained.call_args.kwargs["config"], fixture.config)

    def test_nested_text_context_is_used_for_qwen3_5(self):
        for maximum, expected in [(262144, 32768), (2048, 2048)]:
            with self.subTest(maximum=maximum), patch.object(server, "NETWORK", SimpleNamespace(
                    config=SimpleNamespace(text_config=SimpleNamespace(max_position_embeddings=maximum)))):
                self.assertEqual(server.context_window(), expected)

    def test_chat_validation_rejects_unbounded_and_unsupported_inputs(self):
        valid = {"messages": [{"role": "user", "content": "hello"}]}
        self.assertEqual(server.validate_chat(valid)[1], 256)
        for extra in [{"max_tokens": 0}, {"max_tokens": True}, {"max_tokens": 4097}, {"stream": "yes"}, {"truncate": 1}, {"messages": [{"role": "system", "content": "only"}]}, {"temperature": float("nan")}, {"model": "other/model"}, {"messages": [{"role": "tool", "content": "x"}]}, {"messages": [{"role": "user", "content": "x" * 32769}]}]:
            with self.assertRaises(ValueError):
                server.validate_chat({**valid, **extra})

    def test_truncation_drops_whole_oldest_turns_and_keeps_the_system_prompt(self):
        class Inputs(dict):
            def to(self, device):
                return self
        class Shape:
            def __init__(self, n):
                self.shape = (1, n)
        class Tokenizer:
            chat_template = None
            def __call__(self, text, return_tensors):
                return Inputs(input_ids=Shape(len(text.split())))
        class Config:
            max_position_embeddings = 20
        server.TOKENIZER, server.NETWORK = Tokenizer(), type("Network", (), {"config": Config()})()
        try:
            conversation = [{"role": "system", "content": "be brief"}, {"role": "user", "content": "one two three"}, {"role": "assistant", "content": "four five six"}, {"role": "user", "content": "seven"}]
            with self.assertRaises(ValueError):
                server.prepare(conversation, 8, False)
            _, tokens, dropped = server.prepare(conversation, 8, True)
            self.assertEqual((tokens, dropped), (6, 2))
            with self.assertRaises(ValueError):
                server.prepare([{"role": "user", "content": "x " * 30}], 8, True)
            with self.assertRaises(ValueError):
                server.prepare(conversation, 20, True)
        finally:
            server.TOKENIZER = server.NETWORK = None

    def test_usage_is_recorded_persisted_and_reset_without_prompt_text(self):
        import tempfile
        with tempfile.TemporaryDirectory() as folder:
            server.USAGE_PATH, server.USAGE = os.path.join(folder, "usage.json"), server.empty_usage()
            server.record_usage("api", "ok", 120, 30, 1.5, True)
            server.record_usage("yougori", "busy")
            server.record_usage("api", "rejected")
            usage = server.load_usage()
            self.assertEqual(usage["totals"], {"requests": 2, "prompt_tokens": 120, "completion_tokens": 30, "errors": 1, "rejected": 1, "api": 1, "yougori": 1})
            self.assertEqual(usage["sources"], {"yougori": 1, "api": 1})
            self.assertEqual(sum(h["requests"] for h in usage["hours"].values()), 2)
            self.assertEqual([r["outcome"] for r in usage["recent"]], ["ok", "busy", "rejected"])
            self.assertEqual(set(usage["recent"][0]), {"time", "source", "outcome", "prompt_tokens", "completion_tokens", "seconds", "stream"})
            http = server.Server(("127.0.0.1", 0), server.Handler)
            worker = threading.Thread(target=http.serve_forever, daemon=True)
            worker.start()
            base = "http://127.0.0.1:" + str(http.server_port)
            headers = {"Authorization": "Bearer " + "a" * 64}
            try:
                with self.assertRaises(HTTPError):
                    urlopen(Request(base + "/v1/chat/completions", data=b"{}", headers={"Authorization": "Bearer wrong"}))
                with urlopen(Request(base + "/v1/usage", headers=headers)) as response:
                    self.assertEqual(json.load(response)["totals"]["rejected"], 2)
                with self.assertRaises(HTTPError) as denied:
                    urlopen(Request(base + "/v1/usage/reset", data=b"{}"))
                self.assertEqual(denied.exception.code, 401)
                urlopen(Request(base + "/v1/usage/reset", data=b"{}", headers=headers)).close()
                self.assertEqual(server.load_usage()["totals"]["requests"], 0)
            finally:
                http.shutdown()
                http.server_close()
                worker.join()

    def test_http_auth_status_and_readiness_do_not_load_a_model(self):
        http = server.Server(("127.0.0.1", 0), server.Handler)
        worker = threading.Thread(target=http.serve_forever, daemon=True)
        worker.start()
        base = "http://127.0.0.1:" + str(http.server_port)
        try:
            with self.assertRaises(HTTPError) as denied:
                urlopen(base + "/health")
            self.assertEqual(denied.exception.code, 401)
            headers = {"Authorization": "Bearer " + "a" * 64}
            with urlopen(Request(base + "/health", headers=headers)) as response:
                self.assertEqual(json.load(response)["status"], "installing")
            with self.assertRaises(HTTPError) as loading:
                urlopen(Request(base + "/v1/chat/completions", data=b'{}', headers=headers))
            self.assertEqual(loading.exception.code, 503)
            self.assertNotIn("a" * 64, loading.exception.read().decode())
        finally:
            http.shutdown()
            http.server_close()
            worker.join()

if __name__ == "__main__":
    unittest.main()
