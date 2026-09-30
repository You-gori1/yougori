// @vitest-environment jsdom
import { afterEach, expect, it } from "vitest"
import { changeTour, getTour, startInstructions, stopInstructions } from "./instructions-tour"
import { tourPreviewEnvironment } from "./tour-preview"

afterEach(stopInstructions)

it("shows a temporary overview node only in the home window and removes it on Skip or practice", () => {
  startInstructions()
  const tour = getTour()!
  expect(tourPreviewEnvironment(tour)).toMatchObject({ name: "Tutorial preview", kind: "container", cpuUsage: 0 })
  expect(getTour()?.environmentId).toBeUndefined()
  expect(tourPreviewEnvironment({ ...tour, home: "another-window" })).toBeNull()
  stopInstructions()
  expect(tourPreviewEnvironment(getTour())).toBeNull()
  startInstructions(); changeTour({ step: "create-open" })
  expect(tourPreviewEnvironment(getTour())).toBeNull()
  changeTour({ step: "done", environmentId: "real-container" }); stopInstructions()
  expect(tourPreviewEnvironment(getTour())).toBeNull()
  expect(getTour()?.environmentId).toBe("real-container")
})
