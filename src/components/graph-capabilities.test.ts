import { describe, expect, it } from "vitest"
import { allCapabilities, capabilityIssue, endpointPair, savedSetupForPublication } from "./graph-capabilities"
import type { Environment } from "@/types/platform"

describe("live internet cable", () => {
  it("only exposes optional capabilities, not automatic allocation", () => {
    expect(allCapabilities.map(item => item.capability)).toEqual(["internet", "pc"])
  })
  for (const kind of ["container", "microVm", "fullVm"] as const) {
    for (const status of ["running", "paused", "stopped"] as const) {
      it(`allows ${kind} while ${status}`, () => {
        const env = { name: "Test", kind, provider: kind === "container" ? "yougoriOci" : "qemu", status } as Environment
        expect(capabilityIssue(env, "internet")).toBeNull()
      })
    }
  }
  it("supports the independent microVM internet policy", () => {
    const env = { name: "Test", kind: "microVm", provider: "qemu", status: "running" } as Environment
    expect(capabilityIssue(env, "internet")).toBeNull()
  })
  it("blocks unfinished creation", () => {
    const env = { name: "Test", kind: "container", provider: "yougoriOci", status: "provisioning" } as Environment
    expect(capabilityIssue(env, "internet")).toContain("ready")
  })
})

describe("saved setup connections", () => {
  it("shows a CLI proxy publication on its saved domain card", () => {
    const preset = { id: "preset-1", credentialEnvironmentId: "public-presets", port: 5281, hostPort: 5281, hostname: "crm.example.com" }
    const publication = { id: "pub-1", environmentId: "env-test", kind: "cloudflare" as const, port: 43119, hostPort: 5281, urls: ["https://crm.example.com"], status: "active", message: "", cloudflareAccount: true }
    expect(savedSetupForPublication(publication, [preset])).toEqual(preset)
    expect(savedSetupForPublication({ ...publication, urls: ["https://other.example.com"] }, [preset])).toBeUndefined()
  })
  it("keeps each saved destination distinct from generic public access", () => {
    const service = { kind: "service" as const, id: "env-test:3000" }
    const preset = { kind: "preset" as const, id: "preset-1" }
    expect(endpointPair(service, preset)).toEqual({ type: "publication", publication: "cloudflare", presetId: "preset-1", environmentId: "env-test", port: 3000 })
    expect(endpointPair(preset, service)).toEqual(endpointPair(service, preset))
  })
})
