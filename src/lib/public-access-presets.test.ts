// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest"

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock("@tauri-apps/api/core", () => ({ invoke }))
afterEach(() => { vi.resetModules(); vi.resetAllMocks(); localStorage.clear(); delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ })

const domain = { id: "3f1c2b8e-4a5d-4e6f-9a7b-1c2d3e4f5a6b", credentialEnvironmentId: "public-presets", port: 5281, hostname: "crm.example.com", hostPort: 5281 }

it("uses the engine's saved domains in the desktop app, including changes made from the CLI", async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} })
  const presets = await import("./public-access-presets")
  localStorage.setItem(presets.publicAccessPresetsKey, "[]")
  const changed = vi.fn()
  window.addEventListener("yougori-public-presets-changed", changed)
  presets.applySavedDomains([domain])
  expect(presets.readPublicAccessPresets()).toEqual([domain])
  presets.applySavedDomains([domain])
  expect(changed).toHaveBeenCalledOnce()
  invoke.mockResolvedValueOnce([])
  await presets.writePublicAccessPresets([domain])
  expect(invoke).toHaveBeenCalledWith("list_saved_domains")
  expect(presets.readPublicAccessPresets()).toEqual([])
  window.removeEventListener("yougori-public-presets-changed", changed)
})

it("moves domains saved by older versions into the engine once", async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} })
  const presets = await import("./public-access-presets")
  localStorage.setItem(presets.publicAccessPresetsKey, JSON.stringify([{ ...domain, credentialEnvironmentId: undefined, environmentId: "public-presets" }]))
  invoke.mockResolvedValueOnce([domain])
  await presets.migrateSavedDomains()
  expect(invoke).toHaveBeenCalledWith("import_saved_domains", { domains: [domain] })
  expect(localStorage.getItem(presets.publicAccessPresetsKey)).toBeNull()
  expect(presets.readPublicAccessPresets()).toEqual([domain])
  await presets.migrateSavedDomains()
  expect(invoke).toHaveBeenCalledOnce()
})

it("keeps using browser storage outside the desktop app", async () => {
  const presets = await import("./public-access-presets")
  await presets.writePublicAccessPresets([domain])
  expect(presets.readPublicAccessPresets()).toEqual([domain])
  expect(invoke).not.toHaveBeenCalled()
})
