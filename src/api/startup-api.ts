import { invoke } from "@tauri-apps/api/core"

export interface StartupHealthCheck {
  port: number
  path: string
  method: "GET" | "POST"
  expected_status: number
  timeout_seconds: number
  bearer_secret?: string | null
  body?: Record<string, unknown> | null
}

const fixtureChecks = new Map<string, StartupHealthCheck>()
function native() { return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window }
function browserFixture() { return !import.meta.env.PROD && (import.meta.env.MODE === "test" || import.meta.env.VITE_YOUGORI_TEST_ADAPTER === "1") }

export const startupApi = {
  async getHealthCheck(environmentId: string): Promise<StartupHealthCheck | null> {
    if (native()) return invoke("get_environment_health_check", { environmentId })
    if (browserFixture()) return structuredClone(fixtureChecks.get(environmentId) ?? null)
    throw new Error("Startup health checks are available in the Yougori app")
  },
  async setHealthCheck(environmentId: string, health: StartupHealthCheck | null): Promise<void> {
    if (native()) { await invoke("set_environment_health_check", { environmentId, health }); return }
    if (!browserFixture()) throw new Error("Startup health checks are available in the Yougori app")
    if (health) fixtureChecks.set(environmentId, structuredClone(health))
    else fixtureChecks.delete(environmentId)
  },
}
