import { describe, expect, it } from "vitest"
import { fittedGuestWindow } from "./fit-guest-window"

const available = { width: 1920, height: 1040 }
const guest = { width: 1024, height: 768 }
describe("fit window to guest desktop", () => {
  it("removes side bars by changing the window, not stretching the picture", () => {
    expect(fittedGuestWindow({ width: 1180, height: 760 }, { width: 1180, height: 680 }, guest, available)).toEqual({ width: 907, height: 760 })
  })
  it("removes top and bottom bars from a portrait window", () => {
    expect(fittedGuestWindow({ width: 760, height: 980 }, { width: 760, height: 900 }, guest, available)).toEqual({ width: 760, height: 650 })
  })
  it("accounts for chrome and minimum size while fitting widescreen guests", () => {
    const result = fittedGuestWindow({ width: 720, height: 500 }, { width: 720, height: 420 }, { width: 1920, height: 1080 }, available)
    expect(result).toEqual({ width: 720, height: 485 })
  })
  it("bounds oversized windows to the available work area", () => {
    const result = fittedGuestWindow({ width: 3000, height: 2000 }, { width: 3000, height: 1920 }, guest, available)
    expect(result).toEqual({ width: 1280, height: 1040 })
  })
  it("rejects invalid geometry and impossible minimum sizes", () => {
    expect(() => fittedGuestWindow(guest, guest, { width: 0, height: 0 }, available)).toThrow("Wait")
    expect(() => fittedGuestWindow(guest, guest, guest, { width: Number.NaN, height: 1040 })).toThrow("Wait")
    expect(() => fittedGuestWindow(guest, guest, { width: 100, height: 3000 }, available)).toThrow("cannot fit")
  })
})
