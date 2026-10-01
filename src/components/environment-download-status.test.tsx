// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { EnvironmentDownloadStatus } from "./environment-download-status"
const { heartbeat, stop, toast } = vi.hoisted(() => ({ heartbeat: vi.fn(), stop: vi.fn(), toast: vi.fn() }))
vi.mock("@/api/environment-download-api", () => ({ environmentDownloadApi: { heartbeat, stop } }))
vi.mock("@/components/ui/toast", () => ({ toastManager: { add: toast } }))
const link = { environmentId: "env-one", active: true, url: "https://example.com/token", domain: null, downloads: 7, sizeBytes: 1024 }
beforeEach(() => { vi.resetAllMocks(); heartbeat.mockResolvedValue([link]); stop.mockResolvedValue(undefined) })
afterEach(() => { cleanup(); vi.useRealTimers() })
it("keeps the lease at app level without requiring a node dialog to stay open", async () => {
  vi.useFakeTimers()
  render(<EnvironmentDownloadStatus />)
  await act(async () => { await Promise.resolve() })
  expect(screen.getByText("1 download link on")).toBeInTheDocument()
  expect(heartbeat).toHaveBeenCalledTimes(1)
  await act(async () => { await vi.advanceTimersByTimeAsync(20_000) })
  expect(heartbeat).toHaveBeenCalledTimes(2)
  cleanup()
  await act(async () => { await vi.advanceTimersByTimeAsync(40_000) })
  expect(heartbeat).toHaveBeenCalledTimes(2)
})
it("refreshes on link changes and revokes all active links explicitly", async () => {
  render(<EnvironmentDownloadStatus />)
  expect(await screen.findByText("1 download link on")).toBeInTheDocument()
  heartbeat.mockResolvedValue([link, { ...link, environmentId: "env-two" }])
  act(() => { window.dispatchEvent(new Event("yougori-download-links-changed")) })
  expect(await screen.findByText("2 download links on")).toBeInTheDocument()
  fireEvent.click(screen.getByRole("button", { name: "Turn off" }))
  await waitFor(() => expect(screen.queryByRole("status")).not.toBeInTheDocument())
  expect(stop.mock.calls).toEqual([["env-one"], ["env-two"]])
})
it("keeps failed revocations visible and reports the error", async () => {
  stop.mockRejectedValue(new Error("Engine unavailable"))
  render(<EnvironmentDownloadStatus />)
  expect(await screen.findByText("1 download link on")).toBeInTheDocument()
  fireEvent.click(screen.getByRole("button", { name: "Turn off" }))
  await waitFor(() => expect(toast).toHaveBeenCalled())
  expect(screen.getByText("1 download link on")).toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Turn off" })).toBeEnabled()
})
