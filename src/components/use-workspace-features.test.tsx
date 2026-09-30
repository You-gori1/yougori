// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { workspaceApi } from "@/api/workspace-api"
import { useWorkspaceFeatures } from "@/components/use-workspace-features"
import type { Environment } from "@/types/platform"
import { publicAccessPresetsKey, publicAccessPresetScope } from "@/lib/public-access-presets"

vi.mock("@/api/workspace-api", () => ({ workspaceApi: { manualPorts: vi.fn(), setManualPort: vi.fn(), services: vi.fn(), savedCloudflare: vi.fn(), publish: vi.fn() } }))
const node = { id: "env-test", name: "Test", kind: "fullVm", status: "stopped" } as Environment
let saved: Record<string, number[]>
beforeEach(() => {
  localStorage.clear()
  saved = { "env-test": [8080] }
  vi.mocked(workspaceApi.manualPorts).mockImplementation(async () => structuredClone(saved))
  vi.mocked(workspaceApi.setManualPort).mockImplementation(async (id, port, present) => {
    saved[id] = present ? [...new Set([...(saved[id] ?? []), port])].sort((a, b) => a - b) : (saved[id] ?? []).filter(p => p !== port)
    return structuredClone(saved)
  })
  vi.mocked(workspaceApi.services).mockResolvedValue({ services: [], publications: [], shares: [], notice: "" })
  vi.mocked(workspaceApi.savedCloudflare).mockResolvedValue({ saved: false, hostname: "", hostPort: null })
  vi.mocked(workspaceApi.publish).mockResolvedValue({ id: "pub-test", environmentId: node.id, port: 8080, kind: "cloudflare", hostPort: 45000, urls: ["https://app.example.com"], status: "active", message: "", cloudflareAccount: true })
})

describe("remembered Cloudflare connections", () => {
  const running = { ...node, status: "running" as const }
  it("keeps saved publication endpoints after stopping and reopening the graph", async () => {
    const publication = { id: "saved-route", environmentId: node.id, port: 3000, kind: "cloudflare" as const, hostPort: 45000, urls: ["https://app.example.com"], status: "active", message: "", cloudflareAccount: true }
    vi.mocked(workspaceApi.services).mockResolvedValue({ services: [], publications: [publication], shares: [], notice: "" })
    const view = renderHook(({ environment }) => useWorkspaceFeatures([environment]), { initialProps: { environment: running as Environment } })
    await waitFor(() => expect(view.result.current.decorated[0]?.workspace?.publications).toHaveLength(1))
    view.rerender({ environment: node })
    expect(view.result.current.decorated[0]?.workspace?.services.some(service => service.port === 3000)).toBe(true)
    expect(view.result.current.decorated[0]?.workspace?.publications[0]?.status).toBe("stopped")
    view.unmount()
    const reopened = renderHook(() => useWorkspaceFeatures([node]))
    await waitFor(() => expect(reopened.result.current.decorated[0]?.workspace?.publications[0]?.urls).toEqual(publication.urls))
    expect(reopened.result.current.decorated[0]?.workspace?.services.some(service => service.port === 3000)).toBe(true)
    expect(workspaceApi.publish).not.toHaveBeenCalled()
  })
  it("connects the exact saved domain selected on the graph", async () => {
    const first = { id: "a92e7be0-285b-413e-bf58-dc43ad13b790", credentialEnvironmentId: publicAccessPresetScope, port: 8080, hostname: "first.example.com", hostPort: 45000 }
    const second = { id: "6766f5a7-1b08-47fa-943c-b34f3a0c4d28", credentialEnvironmentId: publicAccessPresetScope, port: 8080, hostname: "second.example.com", hostPort: 45001 }
    localStorage.setItem(publicAccessPresetsKey, JSON.stringify([first, second]))
    const { result } = renderHook(() => useWorkspaceFeatures([running]))
    await act(() => result.current.connectPublication(node.id, 8080, "cloudflare", second.id))
    expect(workspaceApi.publish).toHaveBeenCalledExactlyOnceWith(node.id, 8080, "cloudflare", second.hostPort, { hostname: second.hostname, presetId: second.id, presetSourceEnvironmentId: publicAccessPresetScope, remember: false, routesReviewed: true })
    expect(result.current.dialog).toBeNull()
    await expect(act(() => result.current.connectPublication(node.id, 3000, "cloudflare", second.id))).rejects.toThrow("different app port")
  })
  it("reconnects the matching node and port without a dialog, including after remount", async () => {
    vi.mocked(workspaceApi.savedCloudflare).mockImplementation(async (id, port) => id === node.id && port === 8080
      ? { saved: true, hostname: "app.example.com", hostPort: 45000 }
      : { saved: false, hostname: "", hostPort: null })
    const first = renderHook(() => useWorkspaceFeatures([running]))
    await act(() => first.result.current.connectPublication(node.id, 8080, "public"))
    expect(first.result.current.dialog).toBeNull()
    expect(workspaceApi.publish).toHaveBeenCalledExactlyOnceWith(node.id, 8080, "cloudflare", 45000, { hostname: "app.example.com", token: undefined, remember: true, routesReviewed: true })
    first.unmount()
    const second = renderHook(() => useWorkspaceFeatures([running]))
    await act(() => second.result.current.connectPublication(node.id, 8080, "cloudflare"))
    expect(second.result.current.dialog).toBeNull()
    expect(workspaceApi.publish).toHaveBeenCalledTimes(2)
    await act(() => second.result.current.connectPublication(node.id, 3000, "public"))
    expect(second.result.current.dialog).toMatchObject({ port: 3000, kind: "cloudflare" })
    await act(() => second.result.current.connectPublication("another-node", 8080, "public"))
    expect(second.result.current.dialog).toMatchObject({ environmentId: "another-node", port: 8080 })
    expect(workspaceApi.publish).toHaveBeenCalledTimes(2)
  })
  it("returns to account setup with the error if saved credentials fail", async () => {
    vi.mocked(workspaceApi.savedCloudflare).mockResolvedValue({ saved: true, hostname: "app.example.com", hostPort: 45000 })
    vi.mocked(workspaceApi.publish).mockRejectedValue(new Error("Token revoked"))
    const { result } = renderHook(() => useWorkspaceFeatures([running]))
    await act(() => result.current.connectPublication(node.id, 8080, "cloudflare"))
    expect(result.current.dialog).toMatchObject({ account: true, error: "Token revoked", kind: "cloudflare" })
    expect(workspaceApi.publish).toHaveBeenCalledTimes(1)
  })
  it("opens setup on vault failure and never falls back to a quick link", async () => {
    vi.mocked(workspaceApi.savedCloudflare).mockRejectedValue(new Error("Vault locked"))
    const { result } = renderHook(() => useWorkspaceFeatures([running]))
    await act(() => result.current.connectPublication(node.id, 8080, "public"))
    expect(result.current.dialog).toMatchObject({ account: true, error: "Vault locked" })
    expect(workspaceApi.publish).not.toHaveBeenCalled()
  })
  it("does not duplicate a pending connection and leaves settings accessible", async () => {
    let resolve!: (value: Awaited<ReturnType<typeof workspaceApi.savedCloudflare>>) => void
    vi.mocked(workspaceApi.savedCloudflare).mockReturnValue(new Promise(done => { resolve = done }))
    const { result } = renderHook(() => useWorkspaceFeatures([running]))
    await act(async () => {
      const first = result.current.connectPublication(node.id, 8080, "public")
      await result.current.connectPublication(node.id, 8080, "public")
      resolve({ saved: true, hostname: "app.example.com", hostPort: 45000 })
      await first
    })
    expect(workspaceApi.publish).toHaveBeenCalledTimes(1)
    expect(result.current.dialog).toBeNull()
    act(() => result.current.openService(node.id, 8080))
    expect(result.current.dialog).toMatchObject({ port: 8080 })
  })
})
afterEach(() => { cleanup(); vi.clearAllMocks(); localStorage.clear() })

describe("shared CLI/desktop manual port declarations", () => {
  it("loads backend ports on stopped VMs and saves/removes through the backend", async () => {
    const { result } = renderHook(() => useWorkspaceFeatures([node]))
    await waitFor(() => expect(result.current.manual[node.id]).toEqual([8080]))
    expect(result.current.decorated[0]?.workspace?.services[0]?.port).toBe(8080)
    await act(() => result.current.addPort(node.id, 3000))
    expect(saved[node.id]).toEqual([3000, 8080])
    expect(result.current.dialog).toEqual({ type: "service", environmentId: node.id, port: 3000 })
    await act(() => result.current.removePort(node.id, 3000))
    expect(saved[node.id]).toEqual([8080])
    expect(result.current.dialog).toBeNull()
  })
  it("migrates old graph ports additively without replacing CLI declarations", async () => {
    localStorage.setItem("yougori.manual-ports.v1", JSON.stringify({ "env-test": [3000, 8080] }))
    const { result } = renderHook(() => useWorkspaceFeatures([node]))
    await waitFor(() => expect(result.current.manual[node.id]).toEqual([3000, 8080]))
    expect(workspaceApi.setManualPort).toHaveBeenCalledExactlyOnceWith(node.id, 3000, true)
    expect(localStorage.getItem("yougori.manual-ports.v1")).toBeNull()
  })
  it("keeps declarations when the backend cannot save them and surfaces the failure", async () => {
    const { result } = renderHook(() => useWorkspaceFeatures([node]))
    await waitFor(() => expect(result.current.manual[node.id]).toEqual([8080]))
    vi.mocked(workspaceApi.setManualPort).mockRejectedValueOnce(new Error("Disk is full"))
    await expect(act(() => result.current.addPort(node.id, 3000))).rejects.toThrow("Disk is full")
    expect(result.current.manual[node.id]).toEqual([8080])
    expect(result.current.dialog).toBeNull()
  })
})
