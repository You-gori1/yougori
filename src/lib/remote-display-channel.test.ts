// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest"
import { RemoteDisplayChannel } from "./remote-display-channel"
const { desktop } = vi.hoisted(() => ({ desktop: vi.fn() }))
vi.mock("@/api/remote-access-api", () => ({ remoteAccessApi: { desktop } }))
afterEach(() => { vi.resetAllMocks(); vi.useRealTimers() })
it("serializes display input and stops reads when the view closes", async () => {
  vi.useFakeTimers()
  desktop.mockImplementation(async (_id, p) => p.action === "read" ? p.offset === 0 ? { data: "UkZC", offset: 3, done: false } : new Promise(() => {}) : {})
  const channel = new RemoteDisplayChannel("env-remote", "display-one", vi.fn())
  const message = vi.fn(); channel.onmessage = message; channel.open()
  channel.send(new Uint8Array([1, 2])); channel.send(new Uint8Array([3]))
  await vi.advanceTimersByTimeAsync(0)
  expect(message).toHaveBeenCalled()
  expect(desktop.mock.calls.filter(([, p]) => p.action === "write").map(([, p]) => p.data)).toEqual(["AQI=", "Aw=="])
  channel.close(); const count = desktop.mock.calls.length
  await vi.advanceTimersByTimeAsync(1000)
  expect(desktop).toHaveBeenCalledTimes(count)
  expect(desktop).toHaveBeenLastCalledWith("env-remote", { action: "close", displayId: "display-one" })
})
it("closes the display when access is revoked", async () => {
  desktop.mockImplementation(async (_id, p) => { if (p.action === "read") throw new Error("Session revoked"); return {} })
  const error = vi.fn(); const channel = new RemoteDisplayChannel("env", "display", error)
  channel.open(); await new Promise(resolve => setTimeout(resolve, 0))
  expect(error).toHaveBeenCalled(); expect(channel.readyState).toBe(3)
  channel.send(new Uint8Array([1])); expect(desktop.mock.calls.some(([, p]) => p.action === "write")).toBe(false)
})
