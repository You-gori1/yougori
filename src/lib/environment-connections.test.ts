import { describe, expect, it } from "vitest"
import { connectionPermissions, isTunnelShared, supportsConnections, supportsSharedConnection } from "./environment-connections"

describe("shared environment connections", () => {
  it("supports local container and VM pairs but not computer branches", () => {
    for (const kind of ["container", "microVm", "fullVm"] as const) expect(supportsConnections({ kind })).toBe(true)
    expect(supportsConnections({ kind: "computerBranch" })).toBe(false)
    expect(supportsConnections({ kind: "cloud", runtime: "ssh://server" })).toBe(true)
  })
  it("offers connection handles for tunnel shares, while older invitations remain unsupported", () => {
    expect(supportsConnections({ kind: "cloud", runtime: "shared://tunnel/owner/share-id" })).toBe(true)
    expect(supportsConnections({ kind: "cloud", runtime: "shared://192.168.1.5:1234" })).toBe(false)
    expect(isTunnelShared({ runtime: "shared://tunnel/owner/share-id" })).toBe(true)
  })
  it("limits cross-computer links to network permissions", () => {
    expect(connectionPermissions(false, true)).toEqual(["network", "ports"])
    expect(connectionPermissions(false)).toContain("files")
    for (const source of ["container", "microVm", "fullVm"] as const) for (const target of ["container", "microVm", "fullVm"] as const) {
      const shared = supportsSharedConnection({ kind: source }, { kind: target })
      expect(shared).toBe(source === "container" && target === "container")
      expect(connectionPermissions(shared)).toEqual(shared ? ["network", "ports", "files", "volumes", "data", "secrets"] : ["network", "ports", "files", "volumes", "data"])
    }
  })
})
