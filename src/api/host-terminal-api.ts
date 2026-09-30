import { run } from "@/api/platform-api"

export interface AgentAccess { state: "missing" | "ready" | "updateAvailable" | "conflict"; path: string; message: string }
export interface HostSessionInfo { sessionId: string; cwd: string }
export interface HostTerminalInfo {
  shell: string; cwd: string; sourceCheckout?: string | null; cliPath: string; elevated: boolean; maxSessions: number
  skill: AgentAccess; sessions: HostSessionInfo[]; agentInstructions: string
  agents: { id: "codex" | "claude" | "gemini"; name: string; available: boolean }[]
}
export interface HostTerminalOutput { data: string; offset: number; done: boolean; exitCode: number | null; truncated: boolean }
export interface HostTerminalRequest {
  sessionId: string; action: "create" | "read" | "write" | "resize" | "close"
  data?: string; offset?: number; cols?: number; rows?: number; cwd?: string
}

// Browser test adapter only; production browsers never execute host commands.
let fixtureSkill: AgentAccess = { state: "missing", path: "C:\\Users\\Test\\Yougori\\Workspace\\skills\\yougori", message: "Install the Yougori skill in your workspace." }
const sessions = new Map<string, { cwd: string; bytes: Uint8Array; input: string; done: boolean }>()
const encode = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes))
const fixtureWorkspace = "C:\\Users\\Test\\Yougori\\Workspace"
export const hostTerminalApi = {
  info() {
    return run<HostTerminalInfo>("get_host_terminal_info", {}, () => ({ shell: "PowerShell", cwd: fixtureWorkspace, sourceCheckout: "C:\\Projects\\yougori", cliPath: "C:\\Program Files\\Yougori\\cli\\yougori-cli.exe", elevated: false, maxSessions: 4, skill: fixtureSkill, sessions: [...sessions].map(([sessionId, s]) => ({ sessionId, cwd: s.cwd })), agentInstructions: "Yougori browser test guide. Use yougori-cli schema.", agents: [{ id: "codex", name: "Codex", available: true }, { id: "claude", name: "Claude", available: false }, { id: "gemini", name: "Gemini", available: false }] }))
  },
  validateEditFolder(path: string) {
    return run<string>("validate_edit_app_folder", { path }, () => {
      if (!path.toLowerCase().includes("yougori")) throw new Error("This folder is not a Yougori source checkout")
      return path
    })
  },
  setup() {
    return run<AgentAccess>("set_up_agent_access", {}, () => {
      fixtureSkill = { ...fixtureSkill, state: "ready", message: "Yougori skill is ready. Start a new Codex session to load it." }
      return fixtureSkill
    })
  },
  terminal(request: HostTerminalRequest) {
    return run<HostTerminalOutput>("host_terminal_action", { request }, () => {
      if (request.action === "create") sessions.set(request.sessionId, { cwd: request.cwd ?? fixtureWorkspace, bytes: new TextEncoder().encode("Yougori browser test terminal — not a host shell\r\nPS> "), input: "", done: false })
      const session = sessions.get(request.sessionId)
      if (request.action === "close") { sessions.delete(request.sessionId); return { data: "", offset: 0, done: true, exitCode: null, truncated: false } }
      if (!session) throw new Error("Host terminal is closed")
      if (request.action === "write") {
        const input = new TextDecoder().decode(Uint8Array.from(atob(request.data ?? ""), c => c.charCodeAt(0)))
        session.input += input
        const output = input.includes("\r") ? `${input}\n[Test adapter received input]\r\nPS> ` : input
        const added = new TextEncoder().encode(output)
        session.bytes = new Uint8Array([...session.bytes, ...added])
      }
      const next = Math.min(session.bytes.length, (request.offset ?? 0) + 32 * 1024)
      return { data: request.action === "read" ? encode(session.bytes.slice(request.offset ?? 0, next)) : "", offset: request.action === "read" ? next : 0, done: session.done, exitCode: null, truncated: false }
    })
  },
  async chooseFolder(): Promise<string | null> {
    if (!("__TAURI_INTERNALS__" in window)) return run("choose_host_terminal_folder", {}, () => "C:\\Projects\\website")
    const { open } = await import("@tauri-apps/plugin-dialog")
    const result = await open({ directory: true, multiple: false, title: "Working folder for a new host terminal" })
    return typeof result === "string" ? result : null
  },
}

// Share startup across StrictMode effects. The view restores this session when opened.
let startup: Promise<void> | undefined
export function startHostCli(): Promise<void> {
  startup ??= (async () => {
    const info = await hostTerminalApi.info()
    if (info.elevated) return // The CLI view explains how to reopen normally.
    if (!info.sessions.some(session => !session.sessionId.startsWith("host-edit-"))) {
      await hostTerminalApi.terminal({ action: "create", sessionId: `host-${crypto.randomUUID()}`, cwd: info.cwd, cols: 100, rows: 30 })
    }
  })().catch(error => { startup = undefined; throw error })
  return startup
}
