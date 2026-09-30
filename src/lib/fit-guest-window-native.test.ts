// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { fitWindowToGuestDisplay } from "./fit-guest-window"

const native = vi.hoisted(() => ({ isMaximized: vi.fn(), innerSize: vi.fn(), outerSize: vi.fn(), setSize: vi.fn(), currentMonitor: vi.fn() }))
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => native,
  currentMonitor: native.currentMonitor,
  LogicalSize: class {
    width: number
    height: number
    constructor(width: number, height: number) { this.width = width; this.height = height }
  },
}))

let display: HTMLDivElement
beforeEach(() => {
  vi.resetAllMocks()
  vi.stubGlobal("__TAURI_INTERNALS__", {})
  vi.stubGlobal("innerWidth", 1180)
  vi.stubGlobal("innerHeight", 760)
  vi.stubGlobal("devicePixelRatio", 1.5)
  native.isMaximized.mockResolvedValue(false)
  native.innerSize.mockResolvedValue({ width: 1770, height: 1140 })
  native.outerSize.mockResolvedValue({ width: 1794, height: 1188 })
  native.currentMonitor.mockResolvedValue({ scaleFactor: 1.5, workArea: { size: { width: 2880, height: 1560 } } })
  native.setSize.mockResolvedValue(undefined)
  display = document.createElement("div")
  display.dataset.guestDisplay = ""
  const canvas = document.createElement("canvas"); canvas.width = 1024; canvas.height = 768
  display.append(canvas); document.body.append(display)
  vi.spyOn(display, "getBoundingClientRect").mockReturnValue({ width: 1180, height: 680 } as DOMRect)
})
afterEach(() => { display.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals() })

it("fits the native window in logical pixels on a scaled monitor without changing the framebuffer", async () => {
  await fitWindowToGuestDisplay()
  expect(native.setSize).toHaveBeenCalledWith(expect.objectContaining({ width: 907, height: 760 }))
  expect(display.querySelector("canvas")?.width).toBe(1024)
  expect(display.querySelector("canvas")?.height).toBe(768)
})

it("asks to restore a maximized window instead of claiming an ignored resize succeeded", async () => {
  native.isMaximized.mockResolvedValue(true)
  await expect(fitWindowToGuestDisplay()).rejects.toThrow("Restore the window")
  expect(native.setSize).not.toHaveBeenCalled()
})

it("does not resize a window after its desktop was closed or switched", async () => {
  native.currentMonitor.mockImplementation(async () => { display.remove(); return null })
  await expect(fitWindowToGuestDisplay()).rejects.toThrow("display changed")
  expect(native.setSize).not.toHaveBeenCalled()
})

it("reports a native window-sizing failure", async () => {
  native.setSize.mockRejectedValue(new Error("Window size refused"))
  await expect(fitWindowToGuestDisplay()).rejects.toThrow("Window size refused")
})
