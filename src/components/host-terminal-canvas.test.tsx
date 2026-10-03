// @vitest-environment jsdom
import { act, cleanup, render } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { hostTerminalApi, type HostTerminalOutput } from "@/api/host-terminal-api"
import { HostTerminalCanvas } from "./host-terminal-canvas"

const terminalState = vi.hoisted(() => ({ input: undefined as ((data: string) => void) | undefined }))
vi.mock("@xterm/xterm", () => ({ Terminal: class {
  cols = 80
  rows = 24
  buffer = { active: { getLine() {} } }
  open() {}
  loadAddon() {}
  focus() {}
  attachCustomKeyEventHandler() {}
  registerLinkProvider() { return { dispose() {} } }
  onData(handler: (data: string) => void) { terminalState.input = handler; return { dispose() { terminalState.input = undefined } } }
  onResize() { return { dispose() {} } }
  write() {}
  writeln() {}
  dispose() {}
} }))
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} } }))
vi.mock("@/api/host-terminal-api", () => ({ hostTerminalApi: { terminal: vi.fn() } }))
vi.mock("@/api/workspace-api", () => ({ workspaceApi: { openUrl: vi.fn() } }))

const output: HostTerminalOutput = { data: "", offset: 0, done: false, truncated: false, exitCode: null }
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(yes => { resolve = yes })
  return { promise, resolve }
}
beforeEach(() => {
  vi.resetAllMocks()
  vi.useFakeTimers()
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} })
  vi.mocked(hostTerminalApi.terminal).mockImplementation(request => request.action === "read" ? new Promise(() => {}) : Promise.resolve(output))
})
afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals() })

const tab = { id: "host-audit-one", name: "Terminal 1", cwd: "/test", state: "starting" as const }
async function mount(command?: string) {
  const onState = vi.fn(), onControls = vi.fn()
  const view = render(<HostTerminalCanvas tab={{ ...tab, command }} active onState={onState} onControls={onControls} />)
  await act(async () => {})
  return { ...view, onState }
}

it("retries a missed read at the same cursor and keeps the existing host shell usable without replaying its command", async () => {
  let reads = 0
  vi.mocked(hostTerminalApi.terminal).mockImplementation(request => {
    if (request.action !== "read") return Promise.resolve(output)
    if (++reads === 1) return Promise.resolve({ ...output, offset: 7 })
    if (reads === 2) return Promise.reject(new Error("Failed to fetch"))
    if (reads === 3) return Promise.resolve({ ...output, offset: 11 })
    return new Promise(() => {})
  })
  const { onState } = await mount("echo one launch")
  await act(async () => { await vi.advanceTimersByTimeAsync(120) })
  await act(async () => { await vi.advanceTimersByTimeAsync(500) })
  await act(async () => { terminalState.input!("echo later\r") })
  const requests = vi.mocked(hostTerminalApi.terminal).mock.calls.map(([request]) => request)
  expect(requests.filter(request => request.action === "read").map(request => request.offset)).toEqual([0, 7, 7])
  expect(requests.filter(request => request.action === "create")).toHaveLength(1)
  expect(requests.filter(request => request.action === "write").map(request => atob(request.data!))).toEqual(["echo one launch\r", "echo later\r"])
  expect(onState).not.toHaveBeenCalledWith(tab.id, "error")
})

it("stops retrying failed reads once the tab is disposed", async () => {
  vi.mocked(hostTerminalApi.terminal).mockImplementation(request => request.action === "read" ? Promise.reject(new Error("request timed out")) : Promise.resolve(output))
  const { unmount } = await mount()
  unmount()
  await act(async () => { await vi.advanceTimersByTimeAsync(4000) })
  const requests = vi.mocked(hostTerminalApi.terminal).mock.calls.map(([request]) => request)
  expect(requests.filter(request => request.action === "read")).toHaveLength(1)
  expect(requests.filter(request => request.action === "close")).toHaveLength(1)
})

it("bounds transport retries and does not retry an ownership or closed-session error", async () => {
  vi.mocked(hostTerminalApi.terminal).mockImplementation(request => request.action === "read" ? Promise.reject(new Error("request timed out")) : Promise.resolve(output))
  const { onState, unmount } = await mount()
  await act(async () => { await vi.advanceTimersByTimeAsync(10_000) })
  expect(vi.mocked(hostTerminalApi.terminal).mock.calls.filter(([request]) => request.action === "read")).toHaveLength(4)
  expect(onState).toHaveBeenLastCalledWith(tab.id, "error")
  unmount()
  vi.mocked(hostTerminalApi.terminal).mockClear()
  vi.mocked(hostTerminalApi.terminal).mockImplementation(request => request.action === "read" ? Promise.reject(new Error("This host terminal belongs to a different client")) : Promise.resolve(output))
  const other = await mount()
  await act(async () => { await vi.advanceTimersByTimeAsync(10_000) })
  expect(vi.mocked(hostTerminalApi.terminal).mock.calls.filter(([request]) => request.action === "read")).toHaveLength(1)
  expect(other.onState).toHaveBeenLastCalledWith(tab.id, "error")
})

it("does not retry a failed write or deliver queued pasted chunks after closing a tab", async () => {
  const write = deferred<HostTerminalOutput>()
  vi.mocked(hostTerminalApi.terminal).mockImplementation(request => request.action === "read" ? new Promise(() => {}) : request.action === "write" ? write.promise : Promise.resolve(output))
  const { unmount } = await mount()
  await act(async () => { terminalState.input!("x".repeat(40_000)) })
  unmount()
  await act(async () => { write.resolve(output); await write.promise })
  expect(vi.mocked(hostTerminalApi.terminal).mock.calls.filter(([request]) => request.action === "write")).toHaveLength(1)
})

it("closes only its owned session when creation finishes after unmount", async () => {
  const created = deferred<HostTerminalOutput>()
  vi.mocked(hostTerminalApi.terminal).mockImplementation(request => request.action === "create" ? created.promise : Promise.resolve(output))
  const { unmount } = await mount("never replay this")
  unmount()
  await act(async () => { created.resolve(output); await created.promise })
  const requests = vi.mocked(hostTerminalApi.terminal).mock.calls.map(([request]) => request)
  expect(requests).toEqual([expect.objectContaining({ sessionId: tab.id, action: "create" }), { sessionId: tab.id, action: "close" }])
})
