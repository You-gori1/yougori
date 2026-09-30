// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { bindGuestKeyboard } from "./guest-keyboard"

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), unlisten: vi.fn() }))
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ listen: native.listen }) }))
let listener: (event: { payload: { token: string; code: string; keysym: number; down: boolean } }) => void
let target: HTMLDivElement
const client = { sendKey: vi.fn(), focus: vi.fn() }
const drain = async () => { await vi.waitFor(() => expect(native.listen).toHaveBeenCalled()); await new Promise(resolve => setTimeout(resolve, 0)) }

beforeEach(() => {
  vi.resetAllMocks()
  vi.stubGlobal("__TAURI_INTERNALS__", {})
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32")
  vi.spyOn(document, "hasFocus").mockReturnValue(true)
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} })
  native.invoke.mockResolvedValue(undefined)
  native.listen.mockImplementation(async (_event, callback) => { listener = callback; return native.unlisten })
  target = document.createElement("div"); document.body.append(target)
  vi.spyOn(target, "getBoundingClientRect").mockReturnValue({ left: 0, top: 80, right: 1200, bottom: 760 } as DOMRect)
})
afterEach(() => { target.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals() })

it("captures only over the display and routes native Windows keys to its own session", async () => {
  const error = vi.fn(), cleanup = bindGuestKeyboard(target, client, error)
  await drain()
  target.dispatchEvent(new Event("pointerenter"))
  await vi.waitFor(() => expect(native.invoke).toHaveBeenLastCalledWith("set_guest_keyboard_capture", expect.objectContaining({ bounds: { left: 0, top: 80, right: 1200, bottom: 760 } })))
  const token = native.invoke.mock.calls.at(-1)![1].token as string
  listener({ payload: { token: "another-view", code: "MetaLeft", keysym: 0xffeb, down: true } })
  expect(client.sendKey).not.toHaveBeenCalled()
  listener({ payload: { token, code: "MetaLeft", keysym: 0xffeb, down: true } })
  expect(client.sendKey).toHaveBeenLastCalledWith(0xffeb, "MetaLeft", true)
  target.dispatchEvent(new Event("pointerleave"))
  await vi.waitFor(() => expect(native.invoke).toHaveBeenLastCalledWith("set_guest_keyboard_capture", { token, bounds: null }))
  expect(client.sendKey).toHaveBeenCalledWith(0xffeb, "MetaLeft", false)
  client.sendKey.mockClear()
  listener({ payload: { token, code: "MetaLeft", keysym: 0xffeb, down: true } })
  expect(client.sendKey).not.toHaveBeenCalled()
  cleanup()
  expect(native.unlisten).toHaveBeenCalled()
  expect(error).not.toHaveBeenCalled()
})

it("releases on blur and never captures while another window is active", async () => {
  const cleanup = bindGuestKeyboard(target, client, vi.fn())
  await drain()
  target.dispatchEvent(new Event("pointerenter"))
  vi.mocked(document.hasFocus).mockReturnValue(false)
  window.dispatchEvent(new Event("blur"))
  await vi.waitFor(() => expect(native.invoke).toHaveBeenLastCalledWith("set_guest_keyboard_capture", expect.objectContaining({ bounds: null })))
  expect(client.sendKey).toHaveBeenCalledWith(0xffec, "MetaRight", false)
  cleanup()
})

it("disables capture after unmount even with an earlier enable request pending", async () => {
  const cleanup = bindGuestKeyboard(target, client, vi.fn())
  await drain()
  target.dispatchEvent(new Event("pointerenter"))
  cleanup()
  await vi.waitFor(() => expect(native.invoke).toHaveBeenLastCalledWith("set_guest_keyboard_capture", expect.objectContaining({ bounds: null })))
})

it("reports a native capture failure instead of claiming keys are isolated", async () => {
  const error = vi.fn(), cleanup = bindGuestKeyboard(target, client, error)
  await drain()
  native.invoke.mockRejectedValue(new Error("hook refused"))
  target.dispatchEvent(new Event("pointerenter"))
  await vi.waitFor(() => expect(error).toHaveBeenCalledWith(expect.stringContaining("hook refused")))
  cleanup()
})
