// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { GraphWorkspaceDialogs } from "./graph-workspace-dialogs"
import type { useWorkspaceFeatures } from "./use-workspace-features"
import { workspaceApi } from "@/api/workspace-api"

vi.mock("@/api/workspace-api", () => ({ workspaceApi: { publish: vi.fn(), share: vi.fn(), chooseFolders: vi.fn() } }))
type Model = ReturnType<typeof useWorkspaceFeatures>
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (reason: Error) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}
function model(): Model {
  return {
    decorated: [{ id: "alpha", name: "Alpha", kind: "container", status: "running", runtime: "ubuntu:24.04", description: "Test fixture", createdAt: "2026-01-01T00:00:00Z", cpuUsage: 0, memoryUsageGb: 0, storageDeltaGb: 0, networkRxMbps: 0,
      resourcePolicy: { cpu: { min: 0.5, preferred: 1, max: 2, current: 1 }, memoryGb: { min: 0.5, preferred: 1, max: 2, current: 1 }, priority: "normal", dynamic: true },
      workspace: { services: [], shares: [], publications: [], notice: "" } }],
    dialog: { type: "service", environmentId: "alpha", port: 3000 },
    setDialog: vi.fn(), refresh: vi.fn().mockResolvedValue(undefined), openShares: vi.fn(), openService: vi.fn(),
    connectPublication: vi.fn(), addPort: vi.fn(), removePort: vi.fn(), manual: {},
  }
}
beforeEach(() => { vi.resetAllMocks(); localStorage.clear(); vi.stubGlobal("PointerEvent", MouseEvent) })
afterEach(() => { cleanup(); vi.unstubAllGlobals() })

describe("workspace operation safety", () => {
  it("keeps publishing visible, prevents duplicate clicks, and unlocks after completion", async () => {
    const pending = deferred<Awaited<ReturnType<typeof workspaceApi.publish>>>()
    vi.mocked(workspaceApi.publish).mockReturnValue(pending.promise)
    const state = model()
    render(<GraphWorkspaceDialogs model={state} />)
    const publish = await screen.findByRole("button", { name: "Connect local network" })
    act(() => { fireEvent.click(publish); fireEvent.click(publish) })
    expect(workspaceApi.publish).toHaveBeenCalledTimes(1)
    expect((screen.getByRole("button", { name: "Done" }) as HTMLButtonElement).disabled).toBe(true)
    expect((screen.getByRole("button", { name: "Close" }) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape", code: "Escape" })
    expect(state.setDialog).not.toHaveBeenCalled()
    await act(async () => {
      pending.resolve({ id: "local-1", environmentId: "alpha", port: 3000, kind: "local", hostPort: 13000, status: "running", urls: [], message: "" })
      await pending.promise
    })
    await waitFor(() => expect((screen.getByRole("button", { name: "Done" }) as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByRole("button", { name: "Done" }))
    expect(state.setDialog).toHaveBeenCalledWith(null)
  })

  it("allows retry after a publishing failure even if the reconciliation also fails", async () => {
    vi.mocked(workspaceApi.publish).mockRejectedValue(new Error("Port already in use"))
    const state = model()
    vi.mocked(state.refresh).mockRejectedValue(new Error("Refresh unavailable"))
    render(<GraphWorkspaceDialogs model={state} />)
    fireEvent.click(await screen.findByRole("button", { name: "Connect local network" }))
    expect((await screen.findByRole("alert")).textContent).toContain("Port already in use")
    await waitFor(() => expect((screen.getByRole("button", { name: "Done" }) as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByRole("button", { name: "Connect local network" }))
    await waitFor(() => expect(workspaceApi.publish).toHaveBeenCalledTimes(2))
  })

  it("does not allow dismissal while a writable host-folder connection is pending", async () => {
    const pending = deferred<Awaited<ReturnType<typeof workspaceApi.share>>>()
    vi.mocked(workspaceApi.chooseFolders).mockResolvedValue(["C:\\approved-work"])
    vi.mocked(workspaceApi.share).mockReturnValue(pending.promise)
    const state = model()
    state.dialog = { type: "shares", environmentId: "alpha" }
    render(<GraphWorkspaceDialogs model={state} />)
    fireEvent.click(await screen.findByRole("button", { name: "Choose folders" }))
    await screen.findByText("C:\\approved-work")
    await waitFor(() => expect((screen.getByRole("button", { name: "Connect selected folders" }) as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByRole("radio", { name: /View & Edit/ }))
    fireEvent.click(screen.getByRole("button", { name: "Connect selected folders" }))
    expect(workspaceApi.share).toHaveBeenCalledWith("alpha", "C:\\approved-work", false)
    expect((screen.getByRole("button", { name: "Done" }) as HTMLButtonElement).disabled).toBe(true)
    await act(async () => { pending.reject(new Error("Share unavailable")); await pending.promise.catch(() => undefined) })
    expect((await screen.findByRole("alert")).textContent).toContain("Share unavailable")
    expect(screen.getByText("C:\\approved-work")).toBeTruthy()
    expect(state.refresh).toHaveBeenCalledTimes(2)
  })
})
