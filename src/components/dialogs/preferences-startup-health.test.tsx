// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { PreferencesStartupHealth } from "./preferences-startup-health"
import { startupApi } from "@/api/startup-api"

vi.mock("@/api/startup-api", () => ({ startupApi: { getHealthCheck: vi.fn(), setHealthCheck: vi.fn() } }))
beforeEach(() => { vi.resetAllMocks(); vi.stubGlobal("PointerEvent", MouseEvent); vi.mocked(startupApi.getHealthCheck).mockResolvedValue(null); vi.mocked(startupApi.setHealthCheck).mockResolvedValue() })
afterEach(() => { cleanup(); vi.unstubAllGlobals() })

describe("startup health configuration", () => {
  it("saves a protected credential reference and harmless selected application probe", async () => {
    render(<PreferencesStartupHealth environmentId="api" />)
    const enabled = screen.getByRole("switch", { name: "Check application health" })
    await waitFor(() => expect(enabled.hasAttribute("data-disabled")).toBe(false))
    fireEvent.click(enabled)
    fireEvent.change(screen.getByLabelText("Application port"), { target: { value: "3002" } })
    fireEvent.change(screen.getByLabelText("Saved secret name (optional)"), { target: { value: "api-probe-secret" } })
    fireEvent.click(screen.getByRole("button", { name: "Save health check" }))
    await waitFor(() => expect(startupApi.setHealthCheck).toHaveBeenCalledWith("api", { port: 3002, path: "/health", method: "GET", expected_status: 200, timeout_seconds: 10, bearer_secret: "api-probe-secret", body: null }))
    expect((await screen.findByText("Health check saved.")).textContent).toBe("Health check saved.")
  })
  it("preserves an existing authenticated POST probe when saving and reports invalid paths without writing", async () => {
    vi.mocked(startupApi.getHealthCheck).mockResolvedValue({ port: 8080, path: "/qualify", method: "POST", expected_status: 200, timeout_seconds: 25, bearer_secret: "qualification-key", body: { domain: "test.example" } })
    render(<PreferencesStartupHealth environmentId="api-post" />)
    await screen.findByDisplayValue("/qualify")
    fireEvent.change(screen.getByLabelText("Health check path"), { target: { value: "//other-host" } })
    fireEvent.click(screen.getByRole("button", { name: "Save health check" }))
    expect(startupApi.setHealthCheck).not.toHaveBeenCalled()
    expect((await screen.findByRole("alert")).textContent).toContain("valid application")
    fireEvent.change(screen.getByLabelText("Health check path"), { target: { value: "/qualify" } })
    fireEvent.click(screen.getByRole("button", { name: "Save health check" }))
    await waitFor(() => expect(startupApi.setHealthCheck).toHaveBeenCalledWith("api-post", expect.objectContaining({ method: "POST", timeout_seconds: 25, body: { domain: "test.example" }, bearer_secret: "qualification-key" })))
  })
})
