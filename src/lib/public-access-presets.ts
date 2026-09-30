export interface PublicAccessPreset { id: string; credentialEnvironmentId: string; port: number; hostname: string; hostPort: number }
export const publicAccessPresetsKey = "yougori.public-access-presets.v1"
export const publicAccessPresetScope = "public-presets"
export const validServicePort = (value: number) => Number.isInteger(value) && value > 0 && value <= 65535 && value !== 7443

// In the desktop app the engine owns saved domains, so the app and the CLI share one list.
// The browser preview and tests keep using localStorage.
const engineOwned = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
let engineList: PublicAccessPreset[] | null = null

function valid(value: unknown): PublicAccessPreset[] {
  if (!Array.isArray(value)) return []
  return value.flatMap(item => {
    if (!item || typeof item !== "object") return []
    const source = item.credentialEnvironmentId ?? item.environmentId
    if (typeof item.id !== "string" || !/^[a-f0-9-]{36}$/i.test(item.id) || typeof source !== "string" || !/^[a-z0-9_-]{1,128}$/i.test(source) ||
      !validServicePort(item.port) || typeof item.hostname !== "string" || !validServicePort(item.hostPort)) return []
    return [{ id: item.id, credentialEnvironmentId: source, port: item.port, hostname: item.hostname, hostPort: item.hostPort } satisfies PublicAccessPreset]
  })
}

function readStored(): PublicAccessPreset[] {
  try { return valid(JSON.parse(localStorage.getItem(publicAccessPresetsKey) ?? "[]")) } catch { return [] }
}

export function readPublicAccessPresets(): PublicAccessPreset[] {
  return engineOwned() && engineList ? engineList : readStored()
}

/** Called with each engine state update; notifies listeners when the list changed (for example from the CLI). */
export function applySavedDomains(list: unknown) {
  const next = valid(list)
  if (JSON.stringify(next) === JSON.stringify(engineList)) return
  engineList = next
  window.dispatchEvent(new Event("yougori-public-presets-changed"))
}

/** Records the list after a change. The engine already stored it in the desktop app, so this re-reads it there. */
export async function writePublicAccessPresets(next: PublicAccessPreset[]) {
  if (!engineOwned()) {
    localStorage.setItem(publicAccessPresetsKey, JSON.stringify(next))
    window.dispatchEvent(new Event("yougori-public-presets-changed"))
    return
  }
  const { invoke } = await import("@tauri-apps/api/core")
  applySavedDomains(await invoke("list_saved_domains"))
}

/** One-time move of domains saved by older versions out of browser storage into the engine. */
export async function migrateSavedDomains() {
  if (!engineOwned() || localStorage.getItem(publicAccessPresetsKey) === null) return
  const { invoke } = await import("@tauri-apps/api/core")
  applySavedDomains(await invoke("import_saved_domains", { domains: readStored() }))
  localStorage.removeItem(publicAccessPresetsKey)
}
