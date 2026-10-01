// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { StrictMode } from "react"
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { ReleaseUpdateNotice, ReleaseUpdateSettings } from "./release-update"
const { check, remindLater, openDownloads } = vi.hoisted(() => ({ check: vi.fn(), remindLater: vi.fn(), openDownloads: vi.fn() }))
vi.mock("@/api/release-api", () => ({ releaseApi: { check, remindLater, openDownloads } }))
const available = { current: "1.0.1", latest: "1.0.2", channel: "preview", platform: "windows-x86_64", updateAvailable: true, downloadAvailable: true, notes: "Fixes", remindLater: false }
beforeEach(() => {
  vi.resetAllMocks()
  check.mockResolvedValue(available)
  remindLater.mockResolvedValue(undefined)
  openDownloads.mockResolvedValue(undefined)
})
afterEach(() => { cleanup(); vi.useRealTimers() })

it("offers the newer release once under StrictMode without opening or installing anything", async () => {
  render(<StrictMode><ReleaseUpdateNotice /></StrictMode>)
  expect(await screen.findByRole("complementary", { name: "Yougori update available" })).toHaveTextContent("Yougori 1.0.2")
  expect(screen.getByText(/Unsigned installation requires your consent/)).toBeVisible()
  expect(check).toHaveBeenCalledTimes(1)
  expect(openDownloads).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole("button", { name: "Get update" }))
  await waitFor(() => expect(openDownloads).toHaveBeenCalledTimes(1))
})

it("defers a release and keeps working even if saving the reminder fails", async () => {
  remindLater.mockRejectedValue(new Error("read only"))
  render(<ReleaseUpdateNotice />)
  fireEvent.click(await screen.findByRole("button", { name: "Later" }))
  expect(screen.queryByRole("complementary")).toBeNull()
  await waitFor(() => expect(remindLater).toHaveBeenCalledExactlyOnceWith("1.0.2"))
  expect(openDownloads).not.toHaveBeenCalled()
})

it.each([
  ["current", { ...available, updateAvailable: false }],
  ["deferred", { ...available, remindLater: true }],
  ["browser preview", null],
])("does not prompt when %s", async (_name, value) => {
  check.mockResolvedValue(value)
  render(<ReleaseUpdateNotice />)
  await act(async () => {})
  expect(screen.queryByRole("complementary")).toBeNull()
})

it("does not display background network errors", async () => {
  check.mockRejectedValue(new Error("offline"))
  render(<ReleaseUpdateNotice />)
  await act(async () => {})
  expect(screen.queryByRole("alert")).toBeNull()
  expect(screen.queryByRole("complementary")).toBeNull()
})

it("rechecks a running app every 12 hours and clears the timer when closed", async () => {
  vi.useFakeTimers()
  check.mockResolvedValueOnce({ ...available, updateAvailable: false }).mockResolvedValue(available)
  const view = render(<ReleaseUpdateNotice />)
  await act(async () => {})
  expect(screen.queryByRole("complementary")).toBeNull()
  await act(async () => { vi.advanceTimersByTime(12 * 60 * 60 * 1000) })
  expect(screen.getByRole("complementary")).toHaveTextContent("1.0.2")
  view.unmount()
  await act(async () => { vi.advanceTimersByTime(24 * 60 * 60 * 1000) })
  expect(check).toHaveBeenCalledTimes(2)
})

it("retries quietly after 15 minutes if the app starts offline", async () => {
  vi.useFakeTimers()
  check.mockRejectedValueOnce(new Error("offline")).mockResolvedValue(available)
  render(<ReleaseUpdateNotice />)
  await act(async () => {})
  expect(screen.queryByRole("complementary")).toBeNull()
  await act(async () => { vi.advanceTimersByTime(15 * 60 * 1000) })
  expect(screen.getByRole("complementary")).toHaveTextContent("1.0.2")
  expect(check).toHaveBeenCalledTimes(2)
})

it("shows an actionable error if the installer page cannot open", async () => {
  openDownloads.mockRejectedValue(new Error("No browser found. Open https://yougori.com/#download"))
  render(<ReleaseUpdateNotice />)
  fireEvent.click(await screen.findByRole("button", { name: "Get update" }))
  expect(await screen.findByRole("alert")).toHaveTextContent("No browser found")
  expect(screen.getByRole("button", { name: "Get update" })).toBeEnabled()
})

it("manual checks bypass reminders and show current, offline and available results", async () => {
  check.mockResolvedValueOnce({ ...available, updateAvailable: false }).mockRejectedValueOnce(new Error("Try again when online")).mockResolvedValueOnce({ ...available, remindLater: true })
  render(<ReleaseUpdateSettings />)
  const button = screen.getByRole("button", { name: "Check for updates" })
  expect(check).not.toHaveBeenCalled()
  fireEvent.click(button)
  expect(await screen.findByRole("status")).toHaveTextContent("1.0.1 is up to date")
  fireEvent.click(button)
  expect(await screen.findByRole("status")).toHaveTextContent("Try again when online")
  fireEvent.click(button)
  expect(await screen.findByRole("button", { name: "Get update" })).toBeEnabled()
  expect(check.mock.calls).toEqual([[true], [true], [true]])
})
