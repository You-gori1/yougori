"""Yougori's single-model CUDA chat/API workload. No remote repository code is executed."""
import importlib.metadata
import json
import hashlib
import os
import queue
import re
import secrets
import signal
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MODEL = os.environ["YOUGORI_MODEL"]
MODEL_REVISION = os.environ.get("YOUGORI_MODEL_REVISION")
TOKEN = os.environ["YOUGORI_MODEL_TOKEN"]
STATE = {"status": "installing", "model": MODEL, "error": None}
GENERATION = threading.Lock()
REQUESTS = threading.BoundedSemaphore(8)
TOKENIZER = NETWORK = TORCH = None
MODEL_DEPENDENCIES = {"transformers": "5.18.0", "accelerate": "1.15.0", "huggingface-hub": "1.33.0"}
# Usage lives beside the model cache so it survives restarts. Prompts and replies are never recorded.
USAGE_PATH = os.path.join(os.environ.get("HF_HOME", "/root/.cache/huggingface"), "yougori-usage.json")
USAGE_LOCK = threading.Lock()
USAGE_HOURS = 90 * 24
USAGE_RECENT = 100
COUNTERS = ("requests", "prompt_tokens", "completion_tokens", "errors", "rejected")
GENERATION_MAX_SECONDS = 90
STREAM_MAX_SECONDS = 300
STREAM_POLL_SECONDS = 1
STREAM_CANCEL_GRACE_SECONDS = 2


def usage_hour_valid(hour):
    return isinstance(hour, str) and 1 <= len(hour) <= 12 and hour.isascii() and hour.isdigit()


def empty_usage():
    return {"version": 1, "since": int(time.time()), "totals": dict.fromkeys(COUNTERS, 0), "sources": {"yougori": 0, "api": 0}, "hours": {}, "recent": []}


def load_usage():
    try:
        with open(USAGE_PATH, encoding="utf-8") as file:
            value = json.load(file)
        def counters_valid(counters):
            return isinstance(counters, dict) and all(type(count) is int and count >= 0 for count in counters.values())
        if (value.get("version") == 1
                and counters_valid(value.get("totals")) and counters_valid(value.get("sources"))
                and isinstance(value.get("hours"), dict) and all(usage_hour_valid(hour) and counters_valid(bucket) for hour, bucket in value["hours"].items())
                and isinstance(value.get("recent"), list)):
            return value
    except (OSError, ValueError, AttributeError):
        pass
    return empty_usage()


def save_usage():
    try:
        temporary = USAGE_PATH + ".tmp"
        with open(temporary, "w", encoding="utf-8") as file:
            json.dump(USAGE, file, separators=(",", ":"))
        os.replace(temporary, USAGE_PATH)
    except OSError:
        pass  # Usage is best effort; it never blocks generation.


USAGE = load_usage()


def record_usage(source, outcome, prompt_tokens=0, completion_tokens=0, seconds=0.0, stream=False):
    """outcome: ok, cancelled, busy, invalid, error or rejected (bad API key)."""
    now = time.time()
    failed = outcome in ("busy", "invalid", "error")
    with USAGE_LOCK:
        hour = str(int(now // 3600))
        bucket = USAGE["hours"].setdefault(hour, dict.fromkeys(COUNTERS, 0))
        for target in (USAGE["totals"], bucket):
            if outcome == "rejected":
                target["rejected"] = target.get("rejected", 0) + 1
                continue
            target["requests"] = target.get("requests", 0) + 1
            target["prompt_tokens"] = target.get("prompt_tokens", 0) + prompt_tokens
            target["completion_tokens"] = target.get("completion_tokens", 0) + completion_tokens
            target["errors"] = target.get("errors", 0) + (1 if failed else 0)
            target[source] = target.get(source, 0) + 1
        if outcome != "rejected":
            USAGE["sources"][source] = USAGE["sources"].get(source, 0) + 1
        oldest = int(now // 3600) - USAGE_HOURS
        for key in [k for k in USAGE["hours"] if not usage_hour_valid(k) or int(k) < oldest]:
            del USAGE["hours"][key]
        USAGE["recent"] = (USAGE["recent"] + [{"time": int(now), "source": source, "outcome": outcome, "prompt_tokens": prompt_tokens,
                                               "completion_tokens": completion_tokens, "seconds": round(seconds, 3), "stream": stream}])[-USAGE_RECENT:]
        save_usage()


def validate_chat(body):
    # "truncate" is a Yougori extension: drop the oldest turns instead of failing when the context is full.
    if not isinstance(body, dict) or set(body) - {"model", "messages", "max_tokens", "temperature", "stream", "truncate"}:
        raise ValueError("Use model, messages, max_tokens, temperature, stream and truncate")
    if body.get("model", MODEL) != MODEL:
        raise ValueError("This endpoint serves " + MODEL)
    stream, truncate = body.get("stream", False), body.get("truncate", False)
    if type(stream) is not bool or type(truncate) is not bool:
        raise ValueError("stream and truncate must be booleans")
    messages = body.get("messages")
    if not isinstance(messages, list) or not 1 <= len(messages) <= 128:
        raise ValueError("Provide 1–128 chat messages")
    total = 0
    for message in messages:
        if not isinstance(message, dict) or set(message) != {"role", "content"}:
            raise ValueError("Messages require role and content")
        if message["role"] not in ("system", "user", "assistant") or not isinstance(message["content"], str):
            raise ValueError("Invalid message role/content")
        total += len(message["content"])
    if total > 32768:
        raise ValueError("Conversation exceeds 32,768 characters; start a new chat")
    if not any(m["role"] == "user" for m in messages):
        raise ValueError("Include at least one user message")
    tokens = body.get("max_tokens", 256)
    temperature = body.get("temperature", 0.7)
    if type(tokens) is not int or not 1 <= tokens <= 4096:
        raise ValueError("max_tokens must be 1–4096")
    if type(temperature) not in (int, float) or not 0 <= temperature <= 2:
        raise ValueError("temperature must be 0–2")
    return messages, tokens, temperature, stream, truncate


def checkpoint():
    """Inspect metadata before downloading weights; never substitute a chat backbone for a custom head."""
    from huggingface_hub import HfApi
    from transformers import AutoConfig, AutoModelForCausalLM
    metadata = HfApi().model_info(MODEL, revision=MODEL_REVISION, files_metadata=True, timeout=30)
    files = {item.rfilename for item in metadata.siblings or []}
    if {"joint_head_config.json", "joint_head.safetensors"} <= files:
        raise RuntimeError(MODEL + " is a structured decision model with a custom prediction head. "
                           "Yougori's model runner serves text chat and cannot run this decision head. "
                           "See https://huggingface.co/" + MODEL + " for its decision API and runner.")
    revision = metadata.sha
    if (not isinstance(revision, str) or not re.fullmatch(r"[a-fA-F0-9]{40}", revision)
            or MODEL_REVISION is not None and revision != MODEL_REVISION):
        raise RuntimeError("Model metadata did not return the requested immutable checkpoint revision")
    config = AutoConfig.from_pretrained(MODEL, revision=revision, trust_remote_code=False)
    if type(config) not in AutoModelForCausalLM._model_mapping:
        raise RuntimeError("The " + config.model_type + " architecture is not supported by Yougori's text chat runner. "
                           "Choose a causal language model with built-in Transformers support and safetensors weights.")
    return config, revision


def verified_snapshot(revision):
    """Download directly into the persistent guest cache and verify pinned weight identities."""
    from huggingface_hub import HfApi, snapshot_download
    metadata = HfApi().model_info(MODEL, revision=revision, files_metadata=True, timeout=30)
    if metadata.sha != revision:
        raise RuntimeError("Model revision changed during download preflight")
    root = snapshot_download(MODEL, revision=revision,
                             allow_patterns=["*.safetensors", "*.json", "*.txt", "*.model", "*.tiktoken"])
    verified = 0
    for item in metadata.siblings or []:
        if not item.rfilename.endswith(".safetensors"):
            continue
        path = os.path.join(root, item.rfilename)
        if not os.path.isfile(path):
            raise RuntimeError("A pinned safetensors weight file is missing from the persistent model cache")
        lfs = getattr(item, "lfs", None)
        expected = (lfs.get("sha256") if isinstance(lfs, dict) else getattr(lfs, "sha256", None)) or getattr(item, "blob_id", None)
        if not isinstance(expected, str) or len(expected) not in (40, 64):
            raise RuntimeError("The repository did not provide a verifiable weight checksum")
        digest = hashlib.sha256() if len(expected) == 64 else hashlib.sha1()
        size = os.path.getsize(path)
        if len(expected) == 40:
            digest.update(("blob " + str(size) + "\0").encode())
        with open(path, "rb") as file:
            while True:
                chunk = file.read(4 * 1024 * 1024)
                if not chunk:
                    break
                digest.update(chunk)
        if not secrets.compare_digest(digest.hexdigest(), expected):
            raise RuntimeError("A downloaded model weight failed checksum verification; do not load this checkpoint")
        verified += 1
    if not verified:
        raise RuntimeError("No verified safetensors weights were found")
    print("Verified " + str(verified) + " safetensors weight files in persistent guest storage at revision " + revision + ".", flush=True)
    return root


def load_model():
    global TOKENIZER, NETWORK, TORCH
    try:
        print("Checking model dependencies...", flush=True)
        threading.Thread(target=report_loading, daemon=True).start()
        required = dict(MODEL_DEPENDENCIES)
        if os.environ.get('YOUGORI_INSTALL_TORCH') == '1':
            try:
                importlib.metadata.version('torch')
            except importlib.metadata.PackageNotFoundError:
                required['torch'] = '2.8.0'
        needed = []
        for package, version in required.items():
            try:
                installed = importlib.metadata.version(package)
            except importlib.metadata.PackageNotFoundError:
                installed = None
            if installed != version:
                needed.append(package + "==" + version)
        if needed:
            print("Installing model dependencies: " + ", ".join(needed), flush=True)
            subprocess.run([sys.executable, "-m", "pip", "install", "--disable-pip-version-check", "--no-cache-dir", *needed], check=True, timeout=1800)
        STATE["status"] = "downloading"
        import torch
        from transformers import AutoModelForCausalLM, AutoTokenizer
        if not torch.cuda.is_available():
            raise RuntimeError("CUDA is unavailable. Check the GPU runtime and NVIDIA driver in Yougori.")
        TORCH = torch
        gpu_count = torch.cuda.device_count()
        print("Checking model compatibility...", flush=True)
        config, revision = checkpoint()
        verified_snapshot(revision)
        print("Downloading tokenizer for " + MODEL + "...", flush=True)
        TOKENIZER = AutoTokenizer.from_pretrained(MODEL, revision=revision, trust_remote_code=False, local_files_only=True)
        STATE["status"] = "loading"
        print("Downloading model weights and loading onto the GPU (cached files are reused)...", flush=True)
        NETWORK = AutoModelForCausalLM.from_pretrained(
            MODEL, config=config, revision=revision, trust_remote_code=False, use_safetensors=True,
            local_files_only=True,
            dtype="auto",
            device_map="balanced" if gpu_count > 1 else {"": 0},
            attn_implementation="eager",
        ).eval()
        gpu_name = torch.cuda.get_device_name(0)
        STATE.update(status="ready", gpu=f"{gpu_count} × {gpu_name}" if gpu_count > 1 else gpu_name,
                     gpuCount=gpu_count, context=context_window(), stream=True, weightsVerified=True, revision=revision)
        print("Model ready on " + STATE["gpu"] + ". Chat and API requests are available.", flush=True)
    except Exception as error:
        # Token values and full Python tracebacks never enter the API response.
        detail = str(error).replace(TOKEN, "[redacted]")
        if os.environ.get("HF_TOKEN"):
            detail = detail.replace(os.environ["HF_TOKEN"], "[redacted]")
        STATE.update(status="error", error=detail[:2000])
        print("Model could not load: " + detail[:2000], file=sys.stderr, flush=True)


def report_loading():
    started = time.monotonic()
    while STATE["status"] not in ("ready", "error"):
        time.sleep(15)
        if STATE["status"] not in ("ready", "error"):
            print("Model startup: " + STATE["status"] + " (" + str(int(time.monotonic() - started)) + "s elapsed)", flush=True)


def context_window():
    config = getattr(NETWORK.config, "text_config", None) or NETWORK.config
    return min(getattr(config, "max_position_embeddings", None) or 4096, 32768)


def render(messages):
    if not TOKENIZER.chat_template:
        return "\n".join(m["role"] + ": " + m["content"] for m in messages) + "\nassistant:"
    try:
        return TOKENIZER.apply_chat_template(messages, tokenize=False, add_generation_prompt=True)
    except Exception:
        # Some templates (for example Gemma) reject the system role; fold it into the first user turn.
        system = "\n\n".join(m["content"] for m in messages if m["role"] == "system")
        rest = [dict(m) for m in messages if m["role"] != "system"]
        if not system or not rest or rest[0]["role"] != "user":
            raise
        rest[0]["content"] = system + "\n\n" + rest[0]["content"]
        return TOKENIZER.apply_chat_template(rest, tokenize=False, add_generation_prompt=True)


def prepare(messages, count, truncate):
    """Tokenize the conversation, dropping the oldest turns when truncate is set. Returns (inputs, tokens, dropped)."""
    context, dropped = context_window(), 0
    if count >= context:
        raise ValueError("max_tokens must be below this model's " + str(context) + "-token context window")
    messages = list(messages)
    while True:
        inputs = TOKENIZER(render(messages), return_tensors="pt")
        tokens = inputs["input_ids"].shape[-1]
        if tokens + count <= context:
            return inputs.to("cuda"), tokens, dropped
        turns = [i for i, m in enumerate(messages) if m["role"] != "system"]
        if not truncate or len(turns) <= 1:
            raise ValueError("Conversation exceeds this model's " + str(context) + "-token context window; shorten it or reduce max_tokens")
        # Remove the oldest turn and anything before the next user message so roles still alternate.
        del messages[turns[0]]
        dropped += 1
        while True:
            rest = [i for i, m in enumerate(messages) if m["role"] != "system"]
            if len(rest) <= 1 or messages[rest[0]]["role"] == "user":
                break
            del messages[rest[0]]
            dropped += 1


class Cancelled:
    def __init__(self, event):
        self.event = event

    def __call__(self, input_ids, scores, **kwargs):
        return TORCH.full((input_ids.shape[0],), self.event.is_set(), dtype=TORCH.bool, device=input_ids.device)


def completion_id():
    return "chatcmpl-" + secrets.token_hex(12)


class GenerationTimeout(RuntimeError):
    """Only server-created, credential-free timeout messages may reach the API."""


def non_streaming_output(inputs, kwargs, meter):
    from transformers import StoppingCriteriaList
    stop, finished, ownership, result = threading.Event(), threading.Event(), threading.Lock(), {}
    detached = False
    def work():
        try:
            with TORCH.inference_mode():
                result["output"] = NETWORK.generate(**inputs, **kwargs, stopping_criteria=StoppingCriteriaList([Cancelled(stop)]))
        except BaseException as error:
            result["error"] = error
        finally:
            with ownership:
                finished.set()
                if detached:
                    GENERATION.release()
    thread = threading.Thread(target=work, daemon=True)
    thread.start()
    thread.join(GENERATION_MAX_SECONDS)
    timed_out = not finished.is_set()
    if timed_out:
        stop.set()
        thread.join(STREAM_CANCEL_GRACE_SECONDS)
    with ownership:
        if not finished.is_set():
            detached = True
            meter["_generation_deferred"] = True
            STATE.update(status="error", error="Model generation did not stop after cancellation. Restart this model before sending more requests.")
    if timed_out:
        raise GenerationTimeout(STATE["error"] if detached else "Model generation exceeded its time limit; shorten the conversation or choose a smaller response.")
    if "error" in result:
        raise result["error"]
    return result["output"]


def generate(body, handler, meter):
    """Returns (status, json) for a regular reply, or None after streaming server-sent events to handler.
    Fills meter with the outcome and token counts for usage tracking."""
    messages, count, temperature, stream, truncate = validate_chat(body)
    meter["stream"] = stream
    if not GENERATION.acquire(blocking=False):
        meter["outcome"] = "busy"
        return 429, {"error": {"message": "The GPU is busy with another response; retry shortly"}}
    try:
        inputs, input_tokens, dropped = prepare(messages, count, truncate)
        meter["prompt_tokens"] = input_tokens
        pad = TOKENIZER.pad_token_id if TOKENIZER.pad_token_id is not None else TOKENIZER.eos_token_id
        kwargs = {"max_new_tokens": count, "max_time": STREAM_MAX_SECONDS if stream else GENERATION_MAX_SECONDS, "do_sample": temperature > 0, "pad_token_id": pad}
        if temperature > 0:
            kwargs["temperature"] = temperature
        extra = {"truncated_messages": dropped, "context_window": context_window()} if truncate else {}
        if stream:
            stream_reply(handler, inputs, input_tokens, count, kwargs, extra, meter)
            return None
        output = non_streaming_output(inputs, kwargs, meter)
        generated = output[0, input_tokens:]
        meter.update(outcome="ok", completion_tokens=len(generated))
        return 200, {"id": completion_id(), "object": "chat.completion", "created": int(time.time()), "model": MODEL,
                     "choices": [{"index": 0, "message": {"role": "assistant", "content": TOKENIZER.decode(generated, skip_special_tokens=True)}, "finish_reason": "stop" if len(generated) < count else "length"}],
                     "usage": {"prompt_tokens": input_tokens, "completion_tokens": len(generated), "total_tokens": input_tokens + len(generated), **extra}}
    except GenerationTimeout as error:
        meter["outcome"] = "error"
        return 504, {"error": {"message": str(error)}}
    except TORCH.cuda.OutOfMemoryError:
        TORCH.cuda.empty_cache()
        meter["outcome"] = "error"
        return 507, {"error": {"message": "Not enough GPU memory. Shorten the conversation or choose a smaller model."}}
    finally:
        # A stalled GPU worker cannot safely be killed from another Python thread.
        # Its finalizer owns this lock until it actually stops; the HTTP worker can
        # return a bounded failure without allowing overlapping GPU generations.
        if not meter.pop("_generation_deferred", False):
            GENERATION.release()


def stream_reply(handler, inputs, input_tokens, count, kwargs, extra, meter):
    from transformers import StoppingCriteriaList, TextIteratorStreamer
    stop, finished, ownership, result = threading.Event(), threading.Event(), threading.Lock(), {}
    detached = False
    streamer = TextIteratorStreamer(TOKENIZER, skip_prompt=True, skip_special_tokens=True, timeout=STREAM_POLL_SECONDS)

    def work():
        try:
            with TORCH.inference_mode():
                result["output"] = NETWORK.generate(**inputs, **kwargs, streamer=streamer, stopping_criteria=StoppingCriteriaList([Cancelled(stop)]))
        except BaseException as error:
            result["error"] = error
            streamer.end()
        finally:
            with ownership:
                finished.set()
                if detached:
                    GENERATION.release()

    ident, created = completion_id(), int(time.time())
    handler.send_response(200)
    handler.send_header("Content-Type", "text/event-stream; charset=utf-8")
    handler.send_header("Cache-Control", "no-store")
    handler.send_header("Connection", "close")
    handler.end_headers()

    disconnected = False
    def send(value):
        nonlocal disconnected
        if disconnected:
            return
        try:
            data = value if isinstance(value, bytes) else json.dumps(value, ensure_ascii=False).encode("utf-8")
            handler.wfile.write(b"data: " + data + b"\n\n")
            handler.wfile.flush()
        except OSError:
            disconnected = True
            stop.set()  # The client disconnected; generation stops at the next token.

    def chunk(delta, finish=None, **more):
        return {"id": ident, "object": "chat.completion.chunk", "created": created, "model": MODEL, "choices": [{"index": 0, "delta": delta, "finish_reason": finish}], **more}

    thread = threading.Thread(target=work, daemon=True)
    thread.start()
    deadline, timed_out = time.monotonic() + STREAM_MAX_SECONDS, False
    send(chunk({"role": "assistant", "content": ""}))
    while not stop.is_set():
        if time.monotonic() >= deadline:
            timed_out = True
            stop.set()
            break
        try:
            text = next(streamer)
        except queue.Empty:
            if finished.is_set():
                break
            continue
        except StopIteration:
            break
        if text:
            send(chunk({"content": text}))
    thread.join(STREAM_CANCEL_GRACE_SECONDS)
    with ownership:
        if not finished.is_set():
            detached = True
            meter["_generation_deferred"] = True
            STATE.update(status="error", error="Model generation did not stop after cancellation. Restart this model before sending more requests.")
    if detached or timed_out:
        meter["outcome"] = "error"
        send({"error": {"message": STATE["error"] if detached else "Model generation exceeded its time limit; shorten the conversation or choose a smaller response."}})
        send(b"[DONE]")
        return
    error = result.get("error")
    meter["outcome"] = "error" if error is not None else "cancelled" if stop.is_set() else "ok"
    if error is None:
        meter["completion_tokens"] = result["output"].shape[-1] - input_tokens
    if error is not None:
        if isinstance(error, TORCH.cuda.OutOfMemoryError):
            TORCH.cuda.empty_cache()
            send({"error": {"message": "Not enough GPU memory. Shorten the conversation or choose a smaller model."}})
        else:
            send({"error": {"message": "Generation failed. Check the model's compatibility and available GPU memory."}})
    else:
        generated = result["output"].shape[-1] - input_tokens
        finish = "stop" if generated < count else "length"
        send(chunk({}, finish, usage={"prompt_tokens": input_tokens, "completion_tokens": generated, "total_tokens": input_tokens + generated, **extra}))
    send(b"[DONE]")


class Handler(BaseHTTPRequestHandler):
    server_version = "YougoriModel/1"
    def setup(self):
        super().setup()
        self.connection.settimeout(120)

    def log_message(self, *args):
        pass  # Never log prompts or credentials.

    def reply(self, status, value):
        data = json.dumps(value, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(data)

    def authorized(self):
        value = self.headers.get("Authorization", "")
        # HTTP header values may contain Latin-1 bytes. compare_digest rejects
        # non-ASCII strings, so malformed keys must fail authentication first.
        return value.isascii() and secrets.compare_digest(value, "Bearer " + TOKEN)

    def do_GET(self):
        if not self.authorized():
            return self.reply(401, {"error": {"message": "A model API token is required"}})
        if self.path == "/health":
            return self.reply(200, dict(STATE))
        if self.path == "/v1/models":
            return self.reply(200, {"object": "list", "data": [{"id": MODEL, "object": "model", "owned_by": "local"}]})
        if self.path == "/v1/usage":
            with USAGE_LOCK:
                usage = json.loads(json.dumps(USAGE))
            return self.reply(200, usage)
        self.reply(404, {"error": {"message": "Endpoint not found"}})

    def do_POST(self):
        # The label only sorts usage into Yougori chat versus API callers; it grants nothing.
        source = "yougori" if self.headers.get("X-Yougori-Client") == "yougori" else "api"
        if not self.authorized():
            if self.path == "/v1/chat/completions":
                record_usage(source, "rejected")
            return self.reply(401, {"error": {"message": "A model API token is required"}})
        if self.path == "/v1/usage/reset":
            global USAGE
            with USAGE_LOCK:
                USAGE = empty_usage()
                save_usage()
            return self.reply(200, {"reset": True})
        if self.path != "/v1/chat/completions":
            return self.reply(404, {"error": {"message": "Endpoint not found"}})
        if STATE["status"] != "ready":
            return self.reply(503, {"error": {"message": STATE.get("error") or "Model is " + STATE["status"]}})
        started, meter = time.monotonic(), {"outcome": "error"}
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= 65536 or self.headers.get("Transfer-Encoding"):
                raise ValueError("Send a JSON body of at most 64 KiB with Content-Length")
            raw = self.rfile.read(length)
            if len(raw) != length:
                raise ValueError("Incomplete request")
            reply = generate(json.loads(raw), self, meter)
            if reply is not None:
                self.reply(*reply)
        except (ValueError, TypeError) as error:
            meter["outcome"] = "invalid"
            self.reply(400, {"error": {"message": str(error).replace(TOKEN, "[redacted]")}})
        except Exception:
            meter["outcome"] = "error"
            self.reply(500, {"error": {"message": "Generation failed. Check the model's compatibility and available GPU memory."}})
        finally:
            record_usage(source, meter["outcome"], meter.get("prompt_tokens", 0), meter.get("completion_tokens", 0), time.monotonic() - started, meter.get("stream", False))


class Server(ThreadingHTTPServer):
    daemon_threads = True
    def process_request(self, request, address):
        if not REQUESTS.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, address)
        except Exception:
            REQUESTS.release()
            raise
    def process_request_thread(self, request, address):
        try:
            super().process_request_thread(request, address)
        finally:
            REQUESTS.release()


def stop(*_):
    # A usage save in progress finishes first; each save replaces the file atomically anyway.
    USAGE_LOCK.acquire(timeout=2)
    os._exit(0)


if __name__ == "__main__":
    # As the container's first process the server would ignore stop signals it does not handle,
    # making every stop wait for the runtime to kill it.
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    server = Server((os.environ.get("YOUGORI_MODEL_BIND", "0.0.0.0"), 8000), Handler)
    threading.Thread(target=load_model, daemon=True).start()
    server.serve_forever()
