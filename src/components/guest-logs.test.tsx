// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { GuestLogs } from "./guest-logs"

const { logs } = vi.hoisted(() => ({ logs: vi.fn() }))
vi.mock("@/api/workspace-api", () => ({ workspaceApi: { logs } }))
afterEach(() => { cleanup(); vi.resetAllMocks(); vi.useRealTimers() })

it("does not show the previous environment's delayed logs after switching", async () => {
  let finish!: (value: string) => void
  logs.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    .mockResolvedValueOnce("Current environment output")
  const view = render(<GuestLogs environmentId="old" active />)
  view.rerender(<GuestLogs environmentId="current" active />)
  expect(await screen.findByText("Current environment output")).toBeInTheDocument()
  await act(async () => { finish("Previous environment output") })
  expect(screen.getByLabelText("Environment logs")).toHaveTextContent("Current environment output")
  expect(screen.queryByText("Previous environment output")).not.toBeInTheDocument()
})

it("ignores an old request when a hidden logs tab becomes active again", async () => {
  let finish!: (value: string) => void
  logs.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    .mockResolvedValueOnce("Fresh output")
  const view = render(<GuestLogs environmentId="model" active />)
  view.rerender(<GuestLogs environmentId="model" active={false} />)
  view.rerender(<GuestLogs environmentId="model" active />)
  expect(await screen.findByText("Fresh output")).toBeInTheDocument()
  await act(async () => { finish("Stale output") })
  expect(screen.getByLabelText("Environment logs")).toHaveTextContent("Fresh output")
})

it("does not overlap refresh requests and stops polling when unmounted", async () => {
  vi.useFakeTimers()
  let finish!: (value: string) => void
  logs.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  const view = render(<GuestLogs environmentId="model" active />)
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }))
  await act(async () => { await vi.advanceTimersByTimeAsync(20000) })
  expect(logs).toHaveBeenCalledTimes(1)
  await act(async () => { finish("Ready") })
  view.unmount()
  await act(async () => { await vi.advanceTimersByTimeAsync(10000) })
  expect(logs).toHaveBeenCalledTimes(1)
})
