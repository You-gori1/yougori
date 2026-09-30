import { run } from "@/api/platform-api"
import type { PlatformState } from "@/types/platform"

export interface RunpodAccount { email: string | null; balance: number | null; spendPerHour: number | null; spendLimit: number | null }
export interface RunpodStatus { installed: boolean; connected: boolean; account?: RunpodAccount; message?: string }
export type Stock = "high" | "medium" | "low" | "none"
export interface RunpodGpu {
  id: string; name: string; vramGb: number | null; securePrice: number | null; communityPrice: number | null
  available: boolean; stock: Stock; locations: { id: string; stock: Stock }[]; amd: boolean
}
export interface RunpodTemplate {
  id: string; name: string; image: string; kind: "gpu" | "cpu" | "amd"; containerDiskGb: number | null; volumeGb: number | null
  mountPath: string; own: boolean; official: boolean
  ports?: { port: number; kind: string; name: string }[]; readme?: string
}
export interface RunpodVolume { id: string; name: string; sizeGb: number; location: string }
export interface RunpodCatalog {
  checkedAt: string; account: RunpodAccount | null; gpus: RunpodGpu[]; locations: { id: string; country: string }[]
  templates: RunpodTemplate[]; volumes: RunpodVolume[]; registries: { id: string; name: string }[]; issues: string[]
}
export interface HubRepo { id: string; title: string; description: string; category: string; owner: string; repo: string; deploys: number; stars: number; tags?: string[] }
export interface HubInput { key: string; name: string; type: string; description?: string; required: boolean; advanced: boolean; default: unknown; options: { label: string; value: unknown }[]; secret: boolean }
export interface HubRepoDetails extends HubRepo { gpuPools: string[]; gpuCount: number; runsOn: string; containerDiskGb?: number; inputs: HubInput[]; presets: { name: string; defaults: Record<string, unknown> }[]; ready: boolean }

export interface PodRequest {
  name: string; compute: "gpu" | "cpu"; gpuId?: string; gpuCount?: number; cloud?: "secure" | "community"; location?: string
  templateId?: string; image?: string; containerDiskGb: number; volumeGb?: number; volumeMountPath?: string; networkVolumeId?: string
  registryAuthId?: string; httpPorts?: number[]; env?: Record<string, string>; maxHourlyUsd?: number | null
  publicIp?: boolean; globalNetworking?: boolean; compliance?: string[]; minCuda?: string
}
export interface EndpointRequest {
  name: string; hubId: string; title?: string; category?: string; env?: Record<string, string>; gpuId?: string; gpuCount?: number
  workersMin?: number; workersMax?: number; idleTimeout?: number; networkVolumeId?: string; location?: string
}
export interface RunpodResources {
  pods: { id: string; name: string; status: string; gpu: string | null; gpuCount: number; hourlyUsd: number | null; image: string; attached: boolean }[]
  endpoints: { id: string; name: string; workersMax: number; workersMin: number; attached: boolean }[]
}
/** What Yougori records about a RunPod pod or endpoint on its node. */
export interface RunpodExtra {
  kind?: "pod" | "endpoint"; compute?: "gpu" | "cpu"; gpuId?: string; gpuCount?: number; cloud?: "secure" | "community"; hourlyUsd?: number | null
  template?: string; containerDiskGb?: number; volumeGb?: number; mountPath?: string; networkVolumeId?: string
  ports?: { port: number; kind: string; name: string }[]; jupyter?: boolean; sshReady?: boolean; sshNote?: string | null; statusReason?: string
  title?: string; category?: string; workersMin?: number; workersMax?: number; idleTimeout?: number
  urls?: { run: string; runsync: string; health: string; openai?: string | null }
  health?: { workers?: Record<string, number>; jobs?: Record<string, number> } | null
}
export const runpodExtra = (deployment: { extra?: Record<string, unknown> } | undefined) => (deployment?.extra ?? {}) as RunpodExtra

export type RunpodAction = "inspect" | "start" | "stop" | "restart" | "delete"

const desktop = (what: string) => () => { throw new Error(`${what} requires Yougori Desktop`) }

export const runpodApi = {
  status: () => run<RunpodStatus>("runpod_status", undefined, () => ({ installed: false, connected: false, message: "Neocloud requires Yougori Desktop" })),
  /** Installs RunPod's tools when needed; with a key, verifies and saves it. */
  connect: (apiKey?: string) => run<RunpodStatus>("runpod_connect", { apiKey: apiKey ?? null }, desktop("Connecting Neocloud")),
  disconnect: () => run<RunpodStatus>("runpod_disconnect", undefined, desktop("Neocloud")),
  catalog: () => run<RunpodCatalog>("runpod_catalog", undefined, desktop("The Neocloud catalogue")),
  template: (id: string) => run<RunpodTemplate>("runpod_template", { id }, desktop("Neocloud templates")),
  searchTemplates: (term: string) => run<RunpodTemplate[]>("runpod_search_templates", { term }, () => []),
  hub: (search?: string) => run<HubRepo[]>("runpod_hub", { search: search ?? null }, () => []),
  hubRepo: (id: string) => run<HubRepoDetails>("runpod_hub_repo", { id }, desktop("The model library")),
  createPod: (request: PodRequest) => run<PlatformState>("runpod_create_pod", { request }, desktop("Creating a pod")),
  createEndpoint: (request: EndpointRequest) => run<PlatformState>("runpod_create_endpoint", { request }, desktop("Creating an endpoint")),
  action: (environmentId: string, action: RunpodAction, confirmation?: string) => run<PlatformState>("runpod_action", { environmentId, action, confirmation: confirmation ?? null }, desktop("Neocloud controls")),
  links: (environmentId: string) => run<{ links: { name: string; port: number; url: string }[]; console: string }>("runpod_links", { environmentId }, () => ({ links: [], console: "https://console.runpod.io/pods" })),
  logs: (environmentId: string) => run<{ source: string; line: string; time?: string }[]>("runpod_logs", { environmentId }, () => []),
  runEndpoint: (environmentId: string, input: Record<string, unknown>) => run<{ ok: boolean; job: Record<string, unknown> }>("runpod_endpoint_run", { environmentId, input }, desktop("Testing an endpoint")),
  resources: () => run<RunpodResources>("runpod_resources", undefined, () => ({ pods: [], endpoints: [] })),
  attach: (kind: "pod" | "endpoint", resourceId: string) => run<PlatformState>("runpod_attach", { kind, resourceId }, desktop("Neocloud")),
  volume: (action: "create" | "resize" | "delete", options: { id?: string; name?: string; location?: string; sizeGb?: number }) =>
    run<unknown>("runpod_volume", { action, id: options.id ?? null, name: options.name ?? null, location: options.location ?? null, sizeGb: options.sizeGb ?? null }, desktop("Network volumes")),
  registry: (action: "create" | "delete", options: { id?: string; name?: string; username?: string; password?: string }) =>
    run<unknown>("runpod_registry", { action, id: options.id ?? null, name: options.name ?? null, username: options.username ?? null, password: options.password ?? null }, desktop("Registry logins")),
}

export const RUNPOD_CONSOLE = "https://console.runpod.io"
/** RunPod's published storage rates (USD per GB each month), September 2026. */
export const STORAGE_RATES = { container: 0.10, volumeRunning: 0.10, volumeStopped: 0.20, network: 0.07 }

export const money = (usd: number | null | undefined, digits = 2) => usd == null ? "—" : `$${usd.toFixed(usd < 1 && digits === 2 ? 2 : digits)}`

/** A plain-language hint of what a GPU's memory is enough for. */
export function goodFor(vramGb: number | null): string {
  if (vramGb == null) return ""
  if (vramGb >= 140) return "The largest open models and multi-node training"
  if (vramGb >= 80) return "70B models (quantized), fine-tuning and fast serving"
  if (vramGb >= 48) return "30B models, Flux and video generation"
  if (vramGb >= 24) return "7–13B models, Stable Diffusion XL and Flux"
  if (vramGb >= 16) return "Small models, SDXL and notebooks"
  return "Light notebooks and experiments"
}

/** A node's RunPod state in plain words. */
export function stateLabel(state: string, product: string): string {
  const s = state.toLowerCase()
  if (product === "serverless") return s === "paused" ? "Paused" : s === "ready" ? "Ready" : s === "creating" ? "Deploying" : state
  if (s === "running") return "Running"
  if (s === "starting" || s === "initializing" || s === "creating") return "Starting"
  if (s === "stopping") return "Stopping"
  if (s === "stopped" || s === "exited") return "Stopped"
  if (s === "deleted") return "Deleted"
  return state
}
