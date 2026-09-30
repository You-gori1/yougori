import { CloudIcon, Globe2Icon, LaptopIcon, NetworkIcon, RadioTowerIcon } from "lucide-react"
import type { EnvironmentServices, PublicationKind } from "@/api/workspace-api"
import type { PublicAccessPreset } from "@/lib/public-access-presets"
import type { Environment } from "@/types/platform"

export type GraphEnvironment = Environment & { workspace?: EnvironmentServices }
export type CapabilityKind = "internet" | "pc"
export type CapabilityEndpoint = { kind: "capability"; id: CapabilityKind } | { kind: "environment"; id: string } | { kind: "service"; id: string } | { kind: "publication"; id: PublicationKind } | { kind: "preset"; id: string }

export const capabilityDefinitions = [
  { capability: "internet", title: "Internet access", icon: Globe2Icon },
] as const

export const pcDefinition = { capability: "pc", title: "My PC files", icon: LaptopIcon } as const
export const allCapabilities = [...capabilityDefinitions, pcDefinition]
export const publicationDefinitions = [
  { kind: "public", title: "Public access", icon: RadioTowerIcon },
  { kind: "cloudflare", title: "Cloudflare Tunnel", icon: CloudIcon },
  { kind: "local", title: "Local network", icon: NetworkIcon },
] as const
export const publicationDestination = (kind: PublicationKind) => kind === "local" ? "local" : "public"
export function savedSetupForPublication(publication: EnvironmentServices["publications"][number], presets: PublicAccessPreset[]) {
  if (publication.kind !== "cloudflare") return undefined
  return presets.find(item => item.hostPort === publication.hostPort && publication.urls.includes(`https://${item.hostname}`))
}
export const publicationDestinations = [
  { kind: "public", title: "Public access / Cloudflare Tunnel", icon: Globe2Icon },
  { kind: "local", title: "Local network", icon: NetworkIcon },
] as const
export const serviceId = (environmentId: string, port: number) => `${environmentId}:${port}`
export function parseServiceId(id: string) {
  const split = id.lastIndexOf(":")
  return { environmentId: id.slice(0, split), port: Number(id.slice(split + 1)) }
}

export function capabilityEnabled(environment: GraphEnvironment, capability: CapabilityKind) {
  if (capability === "pc") return Boolean(environment.workspace?.shares.length)
  if (capability === "internet") return environment.networkAccess ?? false
  return false
}

export function capabilityIssue(environment: Environment, capability: CapabilityKind): string | null {
  if (environment.kind === "cloud") return "Cloud nodes use private node connections only. Their internet and host folders are not managed by Yougori."
  if (capability === "pc") return environment.kind === "computerBranch" ? "Configure folder access in this native branch's settings." : environment.status !== "running" ? `Start ${environment.name} before sharing folders.` : null
  const container = ["yougoriOci", "yougoriCuda"].includes(environment.provider ?? "") && environment.kind === "container"
  if (capability === "internet") {
    if (!container && !(environment.provider === "qemu" && ["fullVm", "microVm"].includes(environment.kind))) return "Internet access is available for containers, MicroVMs and VMs."
    return ["stopped", "running", "paused"].includes(environment.status) ? null : "Wait until the environment is ready before changing internet access."
  }
  return null
}

export function sameEndpoint(a: CapabilityEndpoint | null, b: CapabilityEndpoint | null) {
  return a?.kind === b?.kind && a?.id === b?.id
}

export function endpointPair(a: CapabilityEndpoint, b: CapabilityEndpoint) {
  if (a.kind === "capability" && b.kind === "environment") return { type: "capability" as const, capability: a.id, environmentId: b.id }
  if (a.kind === "environment" && b.kind === "capability") return { type: "capability" as const, capability: b.id, environmentId: a.id }
  if (a.kind === "service" && b.kind === "publication") return { type: "publication" as const, publication: b.id, ...parseServiceId(a.id) }
  if (a.kind === "publication" && b.kind === "service") return { type: "publication" as const, publication: a.id, ...parseServiceId(b.id) }
  if (a.kind === "service" && b.kind === "preset") return { type: "publication" as const, publication: "cloudflare" as const, presetId: b.id, ...parseServiceId(a.id) }
  if (a.kind === "preset" && b.kind === "service") return { type: "publication" as const, publication: "cloudflare" as const, presetId: a.id, ...parseServiceId(b.id) }
  return null
}
