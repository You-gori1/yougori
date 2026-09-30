// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { VncDesktop } from "./guest-desktop"

const clients = vi.hoisted(() => [] as (EventTarget & { disconnect: ReturnType<typeof vi.fn> })[])
vi.mock("@novnc/novnc", () => ({ default: class extends EventTarget {
  disconnect = vi.fn()
  focus = vi.fn()
  constructor() { super(); clients.push(this) }
} }))
vi.mock("@/lib/guest-display-sizing", () => ({ bindGuestDisplaySizing: () => () => {} }))
vi.mock("@/lib/guest-keyboard", () => ({ bindGuestKeyboard: () => () => {} }))
vi.mock("@/components/guest-terminal", () => ({ GuestTerminal: () => null }))
beforeEach(() => { clients.length = 0 })
afterEach(() => { cleanup(); vi.useRealTimers() })

describe("guest display recovery", () => {
  it("reconnects a disconnected display without calling a VM power operation", async () => {
    render(<VncDesktop websocketUrl="ws://127.0.0.1:5900/" password="test" />)
    await waitFor(() => expect(clients).toHaveLength(1))
    act(() => { clients[0]!.dispatchEvent(new Event("connect")) })
    expect(screen.queryByRole("status")).toBeNull()
    act(() => { clients[0]!.dispatchEvent(new CustomEvent("disconnect", { detail: { clean: false } })) })
    expect((await screen.findByRole("alert")).textContent).toContain("connection was interrupted")
    fireEvent.click(screen.getByRole("button", { name: "Reconnect display" }))
    await waitFor(() => expect(clients).toHaveLength(2))
    act(() => { clients[1]!.dispatchEvent(new Event("connect")) })
    expect(screen.queryByRole("alert")).toBeNull()
    expect(clients[0]!.disconnect).toHaveBeenCalled()
  })

  it("preserves useful security errors if disconnection follows", async () => {
    render(<VncDesktop websocketUrl="ws://127.0.0.1:5900/" password="test" />)
    await waitFor(() => expect(clients).toHaveLength(1))
    act(() => {
      clients[0]!.dispatchEvent(new CustomEvent("securityfailure", { detail: { reason: "Display credentials expired" } }))
      clients[0]!.dispatchEvent(new CustomEvent("disconnect", { detail: { clean: false } }))
    })
    expect((await screen.findByRole("alert")).textContent).toContain("Display credentials expired")
  })

  it("replaces an endless connection spinner with a retryable timeout", async () => {
    vi.useFakeTimers()
    render(<VncDesktop websocketUrl="ws://127.0.0.1:5900/" password="test" />)
    await act(async () => { await vi.advanceTimersByTimeAsync(30_000) })
    expect(screen.getByRole("alert").textContent).toContain("did not connect within 30 seconds")
    expect(screen.getByRole("button", { name: "Reconnect display" })).toBeTruthy()
  })
})
