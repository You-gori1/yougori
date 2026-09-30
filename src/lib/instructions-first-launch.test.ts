// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from "vitest"

beforeEach(() => { localStorage.clear(); vi.resetModules(); vi.restoreAllMocks() })

it("opens once on first launch, survives Skip and reload, and allows manual replay", async () => {
  let tour = await import("./instructions-tour")
  tour.startFirstLaunchInstructions()
  expect(tour.getTour()).toMatchObject({ active: true, step: "welcome" })
  const run = tour.getTour()!.run
  tour.startFirstLaunchInstructions()
  expect(tour.getTour()!.run).toBe(run)
  tour.stopInstructions()
  vi.resetModules()
  tour = await import("./instructions-tour")
  tour.startFirstLaunchInstructions()
  expect(tour.getTour()?.active).toBe(false)
  tour.startInstructions()
  expect(tour.getTour()).toMatchObject({ active: true, step: "welcome" })
  expect(tour.getTour()!.run).not.toBe(run)
})

it("does not reopen after transient tour state expires or is removed", async () => {
  const tour = await import("./instructions-tour")
  localStorage.setItem(tour.instructionsSeenKey, "1")
  tour.startFirstLaunchInstructions()
  expect(tour.getTour()).toBeNull()
})

it("recognizes a previously used guide from older versions", async () => {
  const tour = await import("./instructions-tour")
  localStorage.setItem(tour.tourStorageKey, JSON.stringify({ updated: 1 }))
  tour.startFirstLaunchInstructions()
  expect(tour.getTour()).toBeNull()
  expect(localStorage.getItem(tour.instructionsSeenKey)).toBe("1")
})

it("still works once per window when local storage is blocked", async () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("blocked") })
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("blocked") })
  const tour = await import("./instructions-tour")
  tour.startFirstLaunchInstructions()
  const run = tour.getTour()!.run
  tour.stopInstructions()
  tour.startFirstLaunchInstructions()
  expect(tour.getTour()).toMatchObject({ run, active: false })
})
