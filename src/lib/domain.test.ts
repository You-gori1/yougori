import { describe, expect, it } from "vitest"
import seedState from "@/data/seed.json"
import { containerRuntimeCapacity, scheduleResources, storageSummary, validateContainerResourcePolicy, validateResourcePolicy } from "@/lib/domain"
import type { PlatformState, ResourcePolicy } from "@/types/platform"

const validPolicy: ResourcePolicy = {
  cpu: { min: 1, preferred: 4, max: 8, current: 4 },
  memoryGb: { min: 2, preferred: 8, max: 16, current: 8 },
  priority: "normal",
  dynamic: true,
}

describe("resource policy validation", () => {
  it("allows container ceilings above the old cap while protecting host headroom", () => {
    const host = { totalCpu: 16, totalMemoryGb: 32 }
    expect(validateContainerResourcePolicy(validPolicy, host)).toEqual([])
    expect(validateContainerResourcePolicy(validPolicy, { totalCpu: 4, totalMemoryGb: 4 })).toHaveLength(2)
    expect(containerRuntimeCapacity(host)).toEqual({ cpu: 16, memoryGb: 28.375 })
    expect(containerRuntimeCapacity({ totalCpu: 4, totalMemoryGb: 2 }).memoryGb).toBe(0.625)
  })
  it("accepts ordered ranges", () => {
    expect(validateResourcePolicy(validPolicy)).toEqual([])
  })

  it("rejects preferred and maximum values outside their order", () => {
    const invalid: ResourcePolicy = {
      ...validPolicy,
      cpu: { min: 4, preferred: 2, max: 1, current: 2 },
    }
    expect(validateResourcePolicy(invalid)).toHaveLength(2)
  })
})

describe("storage summary", () => {
  it("combines physical layers with measured copy-on-write savings", () => {
    const state = structuredClone(seedState) as PlatformState
    state.host.storageSavedGb = 24
    state.environments.push({
      id: "env-storage", name: "Storage fixture", kind: "fullVm", status: "stopped", runtime: "fixture.qcow2", description: "", createdAt: new Date(0).toISOString(), cpuUsage: 0, memoryUsageGb: 0, storageDeltaGb: 6, networkRxMbps: 0, resourcePolicy: validPolicy,
    })
    const summary = storageSummary(state)
    expect(summary.physical).toBe(6)
    expect(summary.logical).toBe(30)
    expect(summary.ratio).toBe(5)
  })
})

describe("dynamic scheduler", () => {
  it.each(["low", "moderate", "high"] as const)("keeps lightweight defaults valid under %s pressure", (pressure) => {
    const state = structuredClone(seedState) as PlatformState
    state.host.totalCpu = 1
    state.host.totalMemoryGb = 1
    state.host.pressure = pressure
    state.environments.push({
      id: "env-lightweight", name: "Lightweight", kind: "container", status: "running", runtime: "alpine:latest", description: "", createdAt: new Date(0).toISOString(), cpuUsage: 0, memoryUsageGb: 0, storageDeltaGb: 0, networkRxMbps: 0,
      resourcePolicy: {
        cpu: { min: 0.1, preferred: 0.25, max: 0.5, current: 0 },
        memoryGb: { min: 0.125, preferred: 0.25, max: 0.5, current: 0 },
        priority: "normal",
        dynamic: true,
      },
    })
    scheduleResources(state)
    const policy = state.environments[0]?.resourcePolicy
    expect(policy).toBeDefined()
    if (!policy) throw new Error("scheduler removed the lightweight environment")
    expect(policy.cpu.current).toBeGreaterThanOrEqual(policy.cpu.min)
    expect(policy.cpu.current).toBeLessThanOrEqual(policy.cpu.max)
    expect(policy.memoryGb.current).toBeGreaterThanOrEqual(policy.memoryGb.min)
    expect(policy.memoryGb.current).toBeLessThanOrEqual(policy.memoryGb.max)
  })

  it("honors minimums while staying inside the host budget", () => {
    const state = structuredClone(seedState) as PlatformState
    state.host.totalCpu = 8
    state.host.totalMemoryGb = 16
    state.host.pressure = "moderate"
    state.environments.push({
      id: "env-scheduler", name: "Scheduler fixture", kind: "container", status: "running", runtime: "fixture", description: "", createdAt: new Date(0).toISOString(), cpuUsage: 0, memoryUsageGb: 0, storageDeltaGb: 0, networkRxMbps: 0, resourcePolicy: structuredClone(validPolicy),
    })
    scheduleResources(state)
    const running = state.environments.filter((item) => item.status === "running")
    expect(running.every((item) => item.resourcePolicy.cpu.current >= item.resourcePolicy.cpu.min)).toBe(true)
    const floor = running.reduce((sum, item) => sum + item.resourcePolicy.cpu.min, 0)
    const allocated = running.reduce((sum, item) => sum + item.resourcePolicy.cpu.current, 0)
    expect(allocated).toBeLessThanOrEqual(Math.max(floor, state.host.totalCpu * 0.78) + 1)
  })

  it("keeps aggregate container allocations inside the utility guest", () => {
    const state = structuredClone(seedState) as PlatformState
    state.host.totalCpu = 16
    state.host.totalMemoryGb = 32
    state.host.pressure = "low"
    state.environments = Array.from({ length: 5 }, (_, index) => ({
      id: `env-lightweight-${index}`, name: `Lightweight ${index}`, kind: "container" as const, status: "running" as const, runtime: "alpine:latest", description: "", createdAt: new Date(0).toISOString(), cpuUsage: 0, memoryUsageGb: 0, storageDeltaGb: 0, networkRxMbps: 0,
      resourcePolicy: {
        cpu: { min: 0.1, preferred: 0.25, max: 0.5, current: 0 },
        memoryGb: { min: 0.125, preferred: 0.25, max: 0.5, current: 0 },
        priority: "normal" as const,
        dynamic: true,
      },
    }))
    scheduleResources(state)
    expect(state.environments.reduce((sum, item) => sum + item.resourcePolicy.cpu.current, 0)).toBeLessThanOrEqual(16)
    expect(state.environments.reduce((sum, item) => sum + item.resourcePolicy.memoryGb.current, 0)).toBe(1.25)
  })
})
