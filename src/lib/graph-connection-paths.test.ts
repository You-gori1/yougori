import { describe, expect, it } from "vitest"
import { roundedConnectionPath, routeConnections, type ConnectionRoute } from "./graph-connection-paths"

describe("rounded connection routing", () => {
  const source = { x: 100, y: 500, side: "top" as const }
  const target = { x: 300, y: 100, side: "bottom" as const }
  const routes: ConnectionRoute[] = ["a", "b", "c"].map(id => ({ id, sourceKey: "dock", targetKey: "node", source, target }))

  it("fans apart shared sockets and assigns separate rounded lanes", () => {
    const paths = routeConnections(routes).map(line => line.path)
    expect(new Set(paths).size).toBe(3)
    expect(new Set(paths.map(path => path.split("L")[0])).size).toBe(3)
    for (const path of paths) {
      expect(path).toMatch(/^M 100 500 C/)
      expect(path).toMatch(/300 100$/)
      expect(path).toContain("Q")
      expect(path).not.toMatch(/NaN|Infinity/)
    }
  })

  it("keeps routing stable when telemetry reorders environments", () => {
    expect(routeConnections([...routes].reverse())).toEqual(routeConnections(routes))
  })

  it("supports every dock side and coincident endpoints without invalid coordinates", () => {
    for (const side of ["top", "bottom", "left", "right"] as const) {
      for (const end of [target, source]) {
        expect(roundedConnectionPath({ ...source, side }, end)).not.toMatch(/NaN|Infinity/)
      }
    }
  })
})
