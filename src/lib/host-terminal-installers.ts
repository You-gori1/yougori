import type { TerminalInstallerId } from "@/lib/terminal-installers"

// Only fixed commands are sent, in a fresh host terminal selected by the user.
export function hostInstallerCommand(tool: TerminalInstallerId, powershell: boolean): string | null {
  if (tool === "ollama" || tool === "openclaw") return null
  if (tool === "claude") return powershell ? "irm https://claude.ai/install.ps1 | iex" : "curl -fsSL https://claude.ai/install.sh | bash"
  const packages = { codex: "@openai/codex", gemini: "@google/gemini-cli", opencode: "opencode-ai", kilo: "@kilocode/cli" } as const
  const command = `${powershell ? "npm.cmd" : "npm"} install --global ${packages[tool]}`
  return powershell
    ? `if (Get-Command npm.cmd -ErrorAction SilentlyContinue) { ${command} } else { Write-Error 'Install Node.js LTS from https://nodejs.org and reopen Yougori to install this tool.' }`
    : `if command -v npm >/dev/null 2>&1; then ${command}; else printf '%s\\n' 'Install Node.js LTS from https://nodejs.org and reopen Yougori to install this tool.'; fi`
}
