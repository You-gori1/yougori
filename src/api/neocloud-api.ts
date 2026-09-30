import { run } from "@/api/platform-api"
import type { PlatformState } from "@/types/platform"
import providerMetadata from "@/lib/neocloud-providers.json"

export type ComputeProduct = "cpu" | "gpu"
export interface NeocloudOffer {
  provider: string; product: ComputeProduct; offerId: string; name: string
  location: string | null; hourlyPrice: number | null; monthlyPrice: number | null
  currency: string | null; hourlyUsd: number | null; available: boolean | null
  gpuCount: number | null; vramGb: number | null; cpuCores: number | null
  memoryGb: number | null; storageGb: number | null; platform: string | null
}
export interface NeocloudChoice { id: string; name: string }
export interface NeocloudCatalog {
  provider: string; product: ComputeProduct; checkedAt: string; offers: NeocloudOffer[]
  choices: Partial<Record<"locations" | "images" | "sshKeys" | "firewalls" | "platforms" | "subnets" | "projects", NeocloudChoice[]>>
  issues: string[]; source: string
}
export interface NeocloudAccount {
  provider: string; status: "ready" | "cliUnavailable" | "needsContext" | "signInRequired"; message: string
}

export interface NeocloudProvider {
  id: string
  name: string
  cli: string
  available: boolean
  installAvailable?: boolean
  reason: string
  products: ComputeProduct[]
  auth: "apiKey" | "cli"
  authUrl: string; docsUrl: string; pricingUrl: string; consoleUrl: string
  installCommand: string; setupCommand: string; requiresWsl?: boolean
  canStop: boolean; billingNote: string
}

export interface NeocloudRequest {
  provider: string
  product: "cpu" | "gpu" | "serverless"
  name: string
  image: string
  offer: string
  location: string
  diskGb: number
  subnetId?: string
  platform?: string
  sshPublicKey?: string
  firewall?: string
  cpuCores?: number
  memoryGb?: number
  gpuCount?: number
  maxHourlyUsd?: number | null
}

export const neocloudApi = {
  providers() {
    return run<NeocloudProvider[]>("neocloud_providers", undefined, () => providerMetadata.filter(p => p.id === "runpod").map(p => ({ ...p, available: true, reason: p.billingNote })) as NeocloudProvider[])
  },
  install(provider: string) {
    return run<{ provider: string; cli: string; installed: boolean; message: string }>("neocloud_install", { provider }, () => {
      throw new Error("Provider CLI installation requires Yougori Desktop")
    })
  },
  account(provider: string, location?: string) {
    return run<NeocloudAccount>("neocloud_account", { provider, location: location || null }, () => ({ provider, status: "cliUnavailable", message: "Account verification requires Yougori Desktop" }))
  },
  authenticate(provider: string, apiKey: string, location?: string) {
    return run<NeocloudAccount>("neocloud_authenticate", { provider, apiKey, location: location || null }, () => { throw new Error("Sign in requires Yougori Desktop") })
  },
  forgetAccount(provider: string) {
    return run<void>("neocloud_forget_account", { provider }, () => undefined)
  },
  catalog(provider: string, product: ComputeProduct, location?: string) {
    return run<NeocloudCatalog>("neocloud_catalog", { provider, product, location: location || null }, () => ({ provider, product, checkedAt: new Date().toISOString(), offers: [], choices: {}, issues: ["Live catalogues require Yougori Desktop and the provider CLI"], source: "provider CLI" }))
  },
  discover(provider: string, location?: string) {
    return run<unknown>("neocloud_discover", { provider, location: location || null }, () => [])
  },
  create(request: NeocloudRequest, costAcknowledged: boolean) {
    return run<PlatformState>("create_neocloud_environment", { request, costAcknowledged }, () => {
      throw new Error("Provider creation requires Yougori Desktop and the installed provider CLI")
    })
  },
  plan(request: NeocloudRequest) {
    return run<unknown>("neocloud_plan", { request }, () => { throw new Error("Configuration validation requires Yougori Desktop") })
  },
  action(environmentId: string, action: "inspect" | "start" | "stop" | "delete", confirmation?: string) {
    return run<PlatformState>("neocloud_action", { environmentId, action, confirmation: confirmation ?? null }, () => {
      throw new Error("Provider controls require Yougori Desktop")
    })
  },
  recoverId(environmentId: string, resourceId: string) {
    return run<PlatformState>("neocloud_recover_id", { environmentId, resourceId }, () => {
      throw new Error("Provider reconciliation requires Yougori Desktop")
    })
  },
}
