import { validateResourcePolicy } from "@/lib/domain"
import type { EnvironmentKind, ResourcePolicy } from "@/types/platform"

export function resourceControlLimits(kind: EnvironmentKind, hostCpu = 16, hostMemoryGb = 32) {
  return {
    cpu: { min: kind === "container" ? 0.1 : 1, step: kind === "container" ? 0.05 : 1, max: Math.max(1, Math.min(255, Math.floor(hostCpu))) },
    memory: { min: kind === "fullVm" ? 1 : 0.125, step: 0.125, max: Math.floor(Math.min(1024, hostMemoryGb) * 8) / 8 },
  }
}

export function resourceControlErrors(policy: ResourcePolicy, kind: EnvironmentKind, hostCpu?: number, hostMemoryGb?: number): string[] {
  return errorsForLimits(policy, resourceControlLimits(kind, hostCpu, hostMemoryGb))
}

// Existing environments may retain older, smaller minimums. New environments
// use the current creation presets, with the same host-sized resource ceilings.
export function creationResourceLimits(kind: EnvironmentKind, hostCpu = 16, hostMemoryGb = 32) {
  return {
    cpu: { min: kind === "container" ? 0.5 : 1, step: kind === "container" ? 0.05 : 1, max: Math.max(1, Math.min(255, Math.floor(hostCpu))) },
    memory: { min: kind === "container" ? 0.5 : 1, step: 0.125, max: Math.floor(Math.min(1024, hostMemoryGb) * 8) / 8 },
  }
}

export function creationResourceErrors(policy: ResourcePolicy, kind: EnvironmentKind, hostCpu?: number, hostMemoryGb?: number): string[] {
  return errorsForLimits(policy, creationResourceLimits(kind, hostCpu, hostMemoryGb))
}

function errorsForLimits(policy: ResourcePolicy, limits: ReturnType<typeof resourceControlLimits>): string[] {
  const errors = validateResourcePolicy(policy)
  for (const [name, range, limit] of [["CPU", policy.cpu, limits.cpu], ["Memory", policy.memoryGb, limits.memory]] as const) {
    if (range.min < limit.min) errors.push(`${name} minimum must be at least ${name === "Memory" ? `${limit.min} GB` : limit.min}.`)
    if (range.max > limit.max) errors.push(`${name} maximum cannot exceed ${name === "Memory" ? `${limit.max} GB` : `${limit.max} CPUs`} for this environment.`)
    if ([range.min, range.preferred, range.max].some(value => Number.isFinite(value) && Math.abs(value / limit.step - Math.round(value / limit.step)) > 1e-6)) errors.push(`${name} values must use increments of ${name === "Memory" ? `${limit.step} GB` : limit.step}.`)
  }
  return errors
}

// Keep the runtime's persisted policy format compatible with existing backups.
// Equal bounds make allocation fixed even when the runtime reconciles limits.
export function fixedResourcePolicy(policy: ResourcePolicy): ResourcePolicy {
  return {
    ...policy,
    cpu: { ...policy.cpu, min: policy.cpu.preferred, max: policy.cpu.preferred },
    memoryGb: { ...policy.memoryGb, min: policy.memoryGb.preferred, max: policy.memoryGb.preferred },
  }
}

export function fixedResourceErrors(cpu: number, memoryGb: number, limits: ReturnType<typeof resourceControlLimits>): string[] {
  const errors: string[] = []
  for (const [label, value, limit, unit] of [["CPU", cpu, limits.cpu, "CPUs"], ["Memory", memoryGb, limits.memory, "GB"]] as const) {
    if (!Number.isFinite(value)) errors.push(`Enter a number for ${label.toLowerCase()}.`)
    else if (value < limit.min || value > limit.max) errors.push(`${label} must be between ${limit.min} and ${limit.max} ${unit}.`)
    else if (Math.abs(value / limit.step - Math.round(value / limit.step)) > 1e-6) errors.push(`${label} must use increments of ${limit.step} ${unit}.`)
  }
  return errors
}

/** A configuration section stored by the popup's single Save changes button. */
export interface SectionSave {
  /** The user edited this section and it differs from what is saved. */
  changed(): boolean
  /** Why the edit cannot be saved right now, or null. Checked before anything is saved. */
  problem(): string | null
  save(): Promise<void>
}
