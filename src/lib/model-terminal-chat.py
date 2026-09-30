"""Interactive chat inside a Yougori model container (Python standard library only)."""
import json
import os
from pathlib import Path
import tempfile
import unicodedata
import urllib.error
import urllib.request

DEFAULT_SYSTEM = "You are a helpful assistant."


def terminal_text(value):
    # Model output is data, never terminal control sequences (including OSC).
    return "".join(c for c in str(value) if c in "\n\t" or not unicodedata.category(c).startswith("C"))


def load_prompt(path):
    try:
        value = json.loads(path.read_text(encoding="utf-8")).get("system", DEFAULT_SYSTEM)
        return value if isinstance(value, str) and len(value) <= 4000 else DEFAULT_SYSTEM
    except (OSError, ValueError, AttributeError):
        return DEFAULT_SYSTEM


def save_prompt(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent, delete=False) as out:
            temporary = out.name
            os.chmod(temporary, 0o600)
            json.dump({"system": value}, out)
        os.replace(temporary, path)
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)


def request(token, path, body=None):
    payload = json.dumps(body).encode("utf-8") if body is not None else None
    if payload is not None and len(payload) > 65536:
        raise ValueError("Conversation is too long. Use /new to start again.")
    req = urllib.request.Request("http://127.0.0.1:8000" + path, data=payload,
        headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"})
    # Never send model credentials through a proxy inherited from the shell.
    client = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with client.open(req, timeout=120) as reply:
            return json.load(reply)
    except urllib.error.HTTPError as error:
        try:
            detail = json.loads(error.read(65536))["error"]["message"]
        except (ValueError, KeyError, TypeError):
            detail = "Model request failed (HTTP " + str(error.code) + ")."
        raise ValueError(str(detail).replace(token, "[redacted]")) from None
    except (urllib.error.URLError, TimeoutError, OSError):
        raise ValueError("Model server unavailable or timed out. Check startup logs, then try again.") from None


def conversation(system):
    return [{"role": "system", "content": system}] if system else []


def chat(token, model, settings):
    system = load_prompt(settings)
    print("Yougori model chat | " + terminal_text(model))
    print("/new clears the conversation | /system changes instructions | /exit returns to the shell")
    print("System prompt: " + terminal_text(system or "(none)"))
    edit = input("System prompt (Enter to keep, /clear for none): ").strip()
    if edit:
        if len(edit) > 4000:
            print("Prompt exceeds 4,000 characters; keeping the previous prompt.")
        else:
            system = "" if edit == "/clear" else edit
            try:
                save_prompt(settings, system)
            except OSError:
                print("Could not save the prompt; using it for this session only.")
    messages = conversation(system)
    while True:
        prompt = input("\nYou > ").strip()
        if not prompt:
            continue
        if prompt == "/exit":
            return
        if prompt == "/new":
            messages = conversation(system)
            print("New conversation.")
            continue
        if prompt == "/system":
            edit = input("New system prompt (Enter to keep, /clear for none): ").strip()
            if not edit:
                continue
            if len(edit) > 4000:
                print("Keep the system prompt within 4,000 characters.")
                continue
            system = "" if edit == "/clear" else edit
            messages = conversation(system)
            try:
                save_prompt(settings, system)
            except OSError:
                print("Could not save the prompt; using it for this session only.")
            print("System prompt updated. Started a new conversation.")
            continue
        pending = messages + [{"role": "user", "content": prompt}]
        if len(pending) > 128 or sum(len(m["content"]) for m in pending) > 32768:
            print("Conversation limit reached. Use /new or a shorter message.")
            continue
        try:
            health = request(token, "/health")
            if health.get("status") != "ready":
                print("Model " + terminal_text(health.get("status", "starting")) + ". Check startup logs and try again.")
                continue
            print("Thinking...", flush=True)
            response = request(token, "/v1/chat/completions", {"model": model, "messages": pending, "max_tokens": 256, "stream": False})
            answer = response["choices"][0]["message"]["content"]
            if not isinstance(answer, str):
                raise ValueError("The model returned an invalid response.")
            messages = pending + [{"role": "assistant", "content": answer}]
            print("\nModel > " + terminal_text(answer))
        except (ValueError, KeyError, IndexError, TypeError) as error:
            print("Error: " + terminal_text(str(error).replace(token, "[redacted]")))


def main():
    token = os.environ.get("YOUGORI_MODEL_TOKEN")
    model = os.environ.get("YOUGORI_MODEL")
    if not token or not model:
        print("Open Chat inside a Yougori Hugging Face model container.")
        return
    try:
        import readline  # Native line editing and arrow-key history, when available.
    except ImportError:
        pass
    print("\033[2J\033[H", end="", flush=True)
    try:
        chat(token, model, Path.home() / ".config" / "yougori" / "model-chat.json")
    except (KeyboardInterrupt, EOFError):
        print("\nChat closed.")


if __name__ == "__main__":
    main()
