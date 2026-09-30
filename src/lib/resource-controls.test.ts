import { describe, expect, it } from "vitest"
import seed from "@/data/seed.json"
import { scheduleResources, validateResourcePolicy } from "@/lib/domain"
import { creationResourceErrors, creationResourceLimits, resourceControlErrors, resourceControlLimits } from "@/lib/resource-controls"
import type { PlatformState, ResourcePolicy } from "@/types/platform"

const microPolicy = (): ResourcePolicy => ({
  cpu: { min: 1, preferred: 1, max: 2, current: 1 },
  memoryGb: { min: 0.125, preferred: 0.25, max: 0.5, current: 0.25 },
  priority: "normal", dynamic: true,
})

describe("resource controls", () => {
  it.each(["container", "microVm", "fullVm"] as const)("offers host-sized creation and editing ranges for %s", kind => {
    const limits = creationResourceLimits(kind, 24, 63.59)
    expect(limits.cpu).toEqual({ min: kind === "container" ? 0.5 : 1, max: 24, step: kind === "container" ? 0.05 : 1 })
    expect(limits.memory).toEqual({ min: kind === "container" ? 0.5 : 1, max: 63.5, step: 0.125 })
    const policy = microPolicy()
    policy.cpu = { min: limits.cpu.min, preferred: 2, max: 24, current: 0 }
    policy.memoryGb = { min: limits.memory.min, preferred: 2, max: 63.5, current: 0 }
    expect(creationResourceErrors(policy, kind, 24, 63.59)).toEqual([])
    policy.memoryGb.min = limits.memory.min - 0.125
    expect(creationResourceErrors(policy, kind, 24, 63.59).join(" ")).toContain("Memory minimum")
    policy.memoryGb.max = 64
    expect(creationResourceErrors(policy, kind, 24, 63.59).join(" ")).toContain("Memory maximum")
    expect(resourceControlLimits("container", 24, 63.59).memory.max).toBe(63.5)
  })
  it("uses 0.125 GB memory steps without applying the container cap to MicroVMs", () => {
    const limits = resourceControlLimits("microVm", 24, 63.5)
    expect(limits.cpu).toEqual({ min: 1, max: 24, step: 1 })
    expect(limits.memory).toEqual({ min: 0.125, max: 63.5, step: 0.125 })
    expect(resourceControlLimits("container", 24, 63.5).memory.max).toBe(63.5)
    expect(resourceControlErrors(microPolicy(), "microVm", 8, 16)).toEqual([])
  })

  it("rejects wrong order, host overflow and invalid increments", () => {
    const policy = microPolicy()
    policy.memoryGb.preferred = 0.75
    expect(resourceControlErrors(policy, "microVm", 8, 16).length).toBeGreaterThan(0)
    policy.memoryGb.max = 1
    expect(resourceControlErrors(policy, "microVm", 8, 16)).toEqual([])
    expect(resourceControlErrors(policy, "microVm", 8, 0.5).join(" ")).toContain("0.5 GB")
    policy.memoryGb.preferred = 0.3
    expect(resourceControlErrors(policy, "microVm", 8, 16).join(" ")).toContain("0.125 GB")
    policy.cpu.preferred = 1.5
    expect(resourceControlErrors(policy, "microVm", 8, 16).join(" ")).toContain("CPU values must use increments of 1")
  })

  it.each([NaN, Infinity, -Infinity])("rejects non-finite resource values (%s)", value => {
    const policy = microPolicy()
    policy.memoryGb.preferred = value
    expect(validateResourcePolicy(policy).length).toBeGreaterThan(0)
  })
})

describe("MicroVM resource scheduling", () => {
  it.each(["low", "moderate", "high"] as const)("preserves boot RAM under %s pressure, including pending downsizing", pressure => {
    const state = structuredClone(seed) as PlatformState
    state.host.totalCpu = 8
    state.host.totalMemoryGb = 16
    state.host.pressure = pressure
    const policy = microPolicy()
    policy.memoryGb.preferred = 0.125
    policy.memoryGb.max = 0.125
    state.environments = [{
      id: "micro", name: "Micro", kind: "microVm", provider: "qemu", status: "running", runtime: "builtin:alpine",
      description: "", createdAt: "", cpuUsage: 0, memoryUsageGb: 0, storageDeltaGb: 0, networkRxMbps: 0,
      resourcePolicy: policy,
    }]
    scheduleResources(state)
    expect(policy.memoryGb.current).toBe(0.25)
    expect(policy.cpu.current).toBeGreaterThanOrEqual(policy.cpu.min)
    expect(policy.cpu.current).toBeLessThanOrEqual(policy.cpu.max)
  })

  it("does not round a 0.15 CPU target down to 0.1", () => {
    const state = structuredClone(seed) as PlatformState
    state.host.totalCpu = 8
    state.host.totalMemoryGb = 16
    state.host.pressure = "low"
    const policy = microPolicy()
    policy.cpu = { min: 0.1, preferred: 0.15, max: 0.5, current: 0.1 }
    state.environments = [{
      id: "container", name: "Container", kind: "container", provider: "yougoriOci", status: "running", runtime: "alpine:latest",
      description: "", createdAt: "", cpuUsage: 0, memoryUsageGb: 0, storageDeltaGb: 0, networkRxMbps: 0,
      resourcePolicy: policy,
    }]
    expect(resourceControlErrors(policy, "container", 8, 16)).toEqual([])
    scheduleResources(state)
    expect(policy.cpu.current).toBe(0.15)
  })
})
