import script from "./model-terminal-chat.py?raw"

// Only bundled code enters this fresh shell. Split the payload into short lines
// so the guest TTY's canonical input limit cannot truncate the bootstrap.
export function modelTerminalChatInput(): string {
  const encoded = btoa(String.fromCharCode(...new TextEncoder().encode(script)))
  const lines = encoded.match(/.{1,512}/g)!.join("\n")
  return `python -c 'import base64;exec(compile(base64.b64decode("""\n${lines}\n"""),"yougori-model-chat","exec"))'\r`
}
