import { defaultOciImage, ociStartupCommand } from "@/data/oci-images"
import { overviewSteps, tourWindowId, type InstructionsTour } from "./instructions-tour"
import type { Environment } from "@/types/platform"

export const tourPreviewId = (tour: InstructionsTour) => `tour-preview-${tour.run}`

// Graph presentation only. Never add this example to platform state or pass it
// to the runtime, service discovery, resource accounting or persistence.
export function tourPreviewEnvironment(tour: InstructionsTour | null): Environment | null {
  if (!tour?.active || tour.home !== tourWindowId || !(overviewSteps as readonly string[]).includes(tour.step)) return null
  return {
    id: tourPreviewId(tour), name: "Tutorial preview", kind: "container", status: "stopped",
    provider: "yougoriOci", runtime: defaultOciImage.value, containerCommand: ociStartupCommand(defaultOciImage),
    description: "A temporary example for the instructions", networkAccess: false,
    createdAt: new Date(tour.updated).toISOString(), cpuUsage: 0, memoryUsageGb: 0, storageDeltaGb: 0, networkRxMbps: 0,
    resourcePolicy: {
      cpu: { min: 0.5, preferred: 0.5, max: 1, current: 0 },
      memoryGb: { min: 0.5, preferred: 0.5, max: 0.5, current: 0 },
      priority: "normal", dynamic: true,
    },
  }
}
