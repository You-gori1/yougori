import { run } from "@/api/platform-api"

export interface GuestApp { id: string; name: string; state: "starting" | "running" | "stopped"; message: string }
export interface GuestAppsStatus { ready: boolean; browserReady: boolean; container?: boolean; containerNotice?: string; installing: boolean; error: string; apps: GuestApp[] }
const fixtures = new Map<string, GuestAppsStatus>()
const fixture = (id: string) => { if (!fixtures.has(id)) fixtures.set(id, { ready: false, browserReady: false, installing: false, error: "", apps: [] }); return fixtures.get(id)! }

export const guestAppsApi = {
  status(environmentId: string) { return run<GuestAppsStatus>("micro_vm_apps", { environmentId, action: "status" }, () => structuredClone(fixture(environmentId))) },
  install(environmentId: string, target: "base" | "browser") { return run<void>("micro_vm_apps", { environmentId, action: "install", package: target }, () => { const state = fixture(environmentId); state.ready = true; if (target === "browser") state.browserReady = true }) },
  launch(environmentId: string, name: string, command: string) {
    const sessionId = `app-${crypto.randomUUID()}`
    return run<GuestApp>("micro_vm_apps", { environmentId, action: "launch", sessionId, name, command }, () => {
      const state = fixture(environmentId)
      if (!state.ready) throw new Error("Install graphical app support first")
      const app: GuestApp = { id: sessionId, name, state: "running", message: "" }; state.apps.push(app); return app
    })
  },
  stop(environmentId: string, sessionId: string) { return run<void>("micro_vm_apps", { environmentId, action: "stop", sessionId }, () => { fixture(environmentId).apps = fixture(environmentId).apps.filter(app => app.id !== sessionId) }) },
  view(environmentId: string, sessionId: string) { return run<{ websocketUrl: string }>("micro_vm_apps", { environmentId, action: "view", sessionId }, () => { if (!fixture(environmentId).apps.some(app => app.id === sessionId)) throw new Error("App session not found"); return { websocketUrl: "ws://127.0.0.1:1/test-app-display" } }) },
  openWindow(environmentId: string, sessionId: string) {
    return run<boolean>("open_micro_vm_app_window", { environmentId, sessionId }, () => {
      const opened = window.open(`/?environment=${encodeURIComponent(environmentId)}&guestApp=${encodeURIComponent(sessionId)}`, "_blank", "noopener,noreferrer")
      void opened
      return true
    })
  },
}
