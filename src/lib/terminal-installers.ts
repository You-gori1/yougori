export const terminalInstallers = [
  { id: "codex", name: "Codex", shortName: "Codex", compactName: "Cx", docs: "https://learn.chatgpt.com/docs/codex/cli" },
  { id: "claude", name: "Claude Code", shortName: "Claude", compactName: "Cl", docs: "https://code.claude.com/docs/en/setup" },
  { id: "gemini", name: "Gemini", shortName: "Gemini", compactName: "G", docs: "https://geminicli.com/docs/get-started/installation/" },
  { id: "ollama", name: "Ollama", shortName: "Ollama", compactName: "Ol", docs: "https://docs.ollama.com/linux" },
  { id: "opencode", name: "OpenCode", shortName: "OpenCode", compactName: "Oc", docs: "https://opencode.ai/docs/" },
  { id: "kilo", name: "Kilo Code", shortName: "Kilo Code", compactName: "K", docs: "https://kilo.ai/docs/code-with-ai/platforms/cli" },
  { id: "openclaw", name: "OpenClaw", shortName: "OpenClaw", compactName: "Ow", docs: "https://docs.openclaw.ai/install" },
] as const

export type TerminalInstallerId = typeof terminalInstallers[number]["id"]
export type TerminalReadyHandler = (sessionId: string, ready: boolean, finished?: boolean) => void

// Only a short launcher crosses the shell's line editor. The complete script
// is staged through the guest agent's non-interactive exec API.
export function terminalInstallerInput(command: string): string {
  if (!/^exec sh '\/tmp\/yougori-install\.[a-zA-Z0-9]+\/install\.sh'$/.test(command) || command.length > 200) {
    throw new Error("Invalid installer launcher. No terminal input was sent.")
  }
  return `${command}\r`
}
