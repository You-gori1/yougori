// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest"
import { bindGuestDisplaySizing } from "./guest-display-sizing"

afterEach(() => vi.restoreAllMocks())

it("uses proportional scaling and only the focused viewer requests guest resolution changes", () => {
  const focused = vi.spyOn(document, "hasFocus").mockReturnValue(true)
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible")
  const client = { scaleViewport: false, resizeSession: false }
  const release = bindGuestDisplaySizing(client, true)
  expect(client).toEqual({ scaleViewport: true, resizeSession: true })
  focused.mockReturnValue(false)
  window.dispatchEvent(new Event("blur"))
  expect(client.resizeSession).toBe(false)
  window.dispatchEvent(new Event("focus"))
  expect(client.resizeSession).toBe(false)
  focused.mockReturnValue(true)
  window.dispatchEvent(new Event("focus"))
  expect(client.resizeSession).toBe(true)
  release()
  expect(client.resizeSession).toBe(false)
})

it("never resizes from a hidden view, even when its document reports focus", () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true)
  const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden")
  const client = { scaleViewport: false, resizeSession: false }
  const release = bindGuestDisplaySizing(client, true)
  expect(client.resizeSession).toBe(false)
  visibility.mockReturnValue("visible")
  document.dispatchEvent(new Event("visibilitychange"))
  expect(client.resizeSession).toBe(true)
  visibility.mockReturnValue("hidden")
  document.dispatchEvent(new Event("visibilitychange"))
  expect(client.resizeSession).toBe(false)
  release()
})

it("fixed-resolution mode and disposed viewers cannot resize the guest", () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true)
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible")
  const client = { scaleViewport: false, resizeSession: true }
  const release = bindGuestDisplaySizing(client, false)
  window.dispatchEvent(new Event("focus"))
  expect(client.resizeSession).toBe(false)
  release()
  const releaseAuto = bindGuestDisplaySizing(client, true)
  expect(client.resizeSession).toBe(true)
  releaseAuto()
  window.dispatchEvent(new Event("focus"))
  document.dispatchEvent(new Event("visibilitychange"))
  expect(client.resizeSession).toBe(false)
  expect(client.scaleViewport).toBe(true)
})
