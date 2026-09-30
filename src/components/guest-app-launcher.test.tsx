// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { guestAppsApi, type GuestAppsStatus } from "@/api/guest-apps-api"
import type { Environment } from "@/types/platform"
import { GuestAppLauncher } from "./guest-app-launcher"

vi.mock("@/api/guest-apps-api", () => ({ guestAppsApi: { status: vi.fn(), install: vi.fn(), launch: vi.fn(), stop: vi.fn(), openWindow: vi.fn() } }))

const environment = (kind: Environment["kind"]): Environment => JSON.parse(JSON.stringify({
  id: "env-a", name: "Alpha", kind, provider: kind === "container" ? "yougoriOci" : "qemu", status: "running",
  runtime: kind === "container" ? "alpine:3.24" : "builtin:alpine", description: "", createdAt: "2026-01-01T00:00:00Z",
  networkAccess: false, gpuAccess: false, cpuUsage: 0, memoryUsageGb: 0, storageDeltaGb: 0, networkRxMbps: 0,
  resourcePolicy: { cpu: { min: 0.5, preferred: 1, max: 2, current: 1 }, memoryGb: { min: 0.5, preferred: 1, max: 2, current: 1 }, priority: "normal", dynamic: false },
}))
const status = (overrides: Partial<GuestAppsStatus> = {}): GuestAppsStatus => ({ ready: true, browserReady: false, installing: false, error: "", apps: [], ...overrides })

beforeEach(() => {
  vi.mocked(guestAppsApi.status).mockResolvedValue(status({ container: true }))
  vi.mocked(guestAppsApi.launch).mockResolvedValue({ id: "app-1", name: "Graphical terminal", state: "running", message: "" })
  vi.mocked(guestAppsApi.openWindow).mockResolvedValue(true)
})
afterEach(() => { cleanup(); vi.resetAllMocks() })

describe("Graphical app launcher", () => {
  it("launches a container app from the container's own image, without offering a browser download", async () => {
    render(<GuestAppLauncher environment={environment("container")} />)
    fireEvent.click(screen.getByRole("button", { name: "Apps" }))
    await screen.findByText(/Run graphical Linux apps in this container/)
    expect(screen.queryByRole("button", { name: "Install Firefox + app support" })).toBeNull()
    expect(screen.getByText(/Nothing is installed inside your container image/)).toBeTruthy()
    expect((screen.getByRole("textbox", { name: "Linux launch command" }) as HTMLInputElement).value).toBe("xterm -fa monospace -fs 12")
    fireEvent.click(screen.getByRole("button", { name: "Launch in new window" }))
    await waitFor(() => expect(guestAppsApi.launch).toHaveBeenCalledWith("env-a", "Graphical terminal", "xterm -fa monospace -fs 12"))
    expect(guestAppsApi.openWindow).toHaveBeenCalledWith("env-a", "app-1")
  })

  it("reports when a container predates graphical support instead of failing silently", async () => {
    vi.mocked(guestAppsApi.status).mockResolvedValue(status({ container: true, containerNotice: "this container was created before graphical app support; recreate it to run apps" }))
    render(<GuestAppLauncher environment={environment("container")} />)
    fireEvent.click(screen.getByRole("button", { name: "Apps" }))
    expect((await screen.findAllByRole("status")).some(element => element.textContent?.includes("recreate it to run apps"))).toBe(true)
  })

  it("keeps the MicroVM browser download and its non-root wording", async () => {
    vi.mocked(guestAppsApi.status).mockResolvedValue(status({ container: false }))
    render(<GuestAppLauncher environment={environment("microVm")} />)
    fireEvent.click(screen.getByRole("button", { name: "Apps" }))
    expect(await screen.findByRole("button", { name: "Install Firefox + app support" })).toBeTruthy()
    expect(screen.getByText(/unprivileged guest user/)).toBeTruthy()
    expect((screen.getByRole("textbox", { name: "App name" }) as HTMLInputElement).value).toBe("Firefox")
  })
})
