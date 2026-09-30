"""Yougori model chat. Python 3.8+, standard library only; no pip install needed.

API keys are read from Yougori at runtime and never saved in this file.
Run this file again to reconnect to the same model. /exit quits the chat.
"""
import json
import os
import shutil
import subprocess
import sys
import urllib.error
import urllib.request

CONFIG = __CONFIG__


def access():
    cli = os.environ.get("YOUGORI_CLI") or shutil.which("yougori") or CONFIG["cli"]
    result = subprocess.run(
        [cli, "model", "access", CONFIG["environment"]],
        capture_output=True, text=True, encoding="utf-8", timeout=30,
    )
    response = json.loads(result.stdout)
    if not response.get("ok"):
        raise RuntimeError(response.get("error", "Cannot read model API access"))
    credentials = response["result"]
    if not credentials.get("apiUrl"):
        raise RuntimeError("Start the environment and enable its local API in Yougori.")
    return credentials


def main():
    credentials = access()
    base_url = credentials["apiUrl"]
    api_key = credentials["apiKey"]
    print("\nYougori ·", credentials["model"], "·", base_url)
    print("/exit to leave · /new to clear the conversation. The model keeps running.\n")
    system = input("System prompt (Enter for default): ").strip() or "You are a helpful assistant."
    messages = [{"role": "system", "content": system}]
    # Never send the localhost API token through a system proxy.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    while True:
        prompt = input("You > ").strip()
        if prompt == "/exit":
            return
        if prompt == "/new":
            messages = messages[:1]
            continue
        if not prompt:
            continue
        request_messages = messages + [{"role": "user", "content": prompt}]
        payload = json.dumps({"model": credentials["model"], "messages": request_messages,
                              "max_tokens": 512, "stream": True, "truncate": True}).encode()
        if len(payload) > 60000:
            print("Conversation is too long. Use /new to start again.")
            continue
        request = urllib.request.Request(base_url.rstrip("/") + "/chat/completions", data=payload,
            headers={"Authorization": "Bearer " + api_key, "Content-Type": "application/json"})
        try:
            answer = ""
            print("Model > ", end="", flush=True)
            with opener.open(request, timeout=180) as response:
                for raw in response:
                    line = raw.decode("utf-8").strip()
                    if not line.startswith("data:"):
                        continue
                    data = line[5:].strip()
                    if data == "[DONE]":
                        break
                    event = json.loads(data)
                    if "error" in event:
                        raise RuntimeError(event["error"].get("message", "Generation failed"))
                    choices = event.get("choices", [])
                    token = choices[0].get("delta", {}).get("content", "") if choices else ""
                    answer += token
                    print("".join(c for c in token if c in "\n\t" or c.isprintable()), end="", flush=True)
            print("\n")
            messages = request_messages + [{"role": "assistant", "content": answer}]
        except (urllib.error.URLError, RuntimeError, ValueError, TimeoutError) as error:
            print("\nRequest failed:", error, "\nCheck model logs in Yougori or try again.")


if __name__ == "__main__":
    try:
        main()
    except (KeyboardInterrupt, EOFError):
        print("\nChat closed. Model remains running.")
    except Exception as error:
        print("Cannot start chat:", error, file=sys.stderr)
        sys.exit(1)
