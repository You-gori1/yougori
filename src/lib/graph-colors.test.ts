import { describe, expect, it } from "vitest"
import { graphColors } from "./graph-colors"

describe("graph colors", () => {
  it("gives nodes distinct muted accents, including beyond the base palette", () => {
    const ids = Array.from({ length: 40 }, (_, i) => `environment-${i}`)
    const colors = graphColors(ids)
    expect(new Set(Object.values(colors)).size).toBe(ids.length)
  })
  it("keeps colors stable when telemetry reorders the graph", () => {
    expect(graphColors(["beta", "alpha", "gamma"])).toEqual(graphColors(["gamma", "beta", "alpha"]))
  })
})
