import type { CloudflareAccountOptions, SavedCloudflareAccount } from "@/api/workspace-api"
import { cloudflareTokenInput } from "@/lib/cloudflare-token"

export interface CloudflareDraft { mode: "quick" | "account"; hostname: string; localPort: string; token: string; remember: boolean; routesReviewed: boolean }
export const emptyCloudflareDraft = (): CloudflareDraft => ({ mode: "quick", hostname: "", localPort: "", token: "", remember: true, routesReviewed: false })

export function savedCloudflareDraft(saved: SavedCloudflareAccount): CloudflareDraft | null {
  if (!saved.saved) return null
  const draft: CloudflareDraft = { ...emptyCloudflareDraft(), mode: "account", hostname: saved.hostname, localPort: String(saved.hostPort ?? ""), routesReviewed: true }
  // Saved credentials were reviewed on successful connection. Validate their
  // public metadata again, and let the native backend retrieve the secret.
  accountRequest(draft)
  return draft
}

export function accountRequest(draft: CloudflareDraft): { hostPort: number; options: CloudflareAccountOptions } {
  const hostname = draft.hostname.trim().toLowerCase()
  if (hostname.length > 253 || !hostname.includes(".") || /^\d+\.\d+\.\d+\.\d+$/.test(hostname) || hostname.endsWith(".localhost") || hostname.endsWith(".local") || hostname.endsWith(".trycloudflare.com") || hostname.split(".").some(label => !/^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(label))) throw new Error("Enter a public hostname such as app.example.com, without https://, a port, or a path.")
  if (!/^\d+$/.test(draft.localPort) || Number(draft.localPort) < 1 || Number(draft.localPort) > 65535 || Number(draft.localPort) === 7443) throw new Error("Choose a local tunnel port from 1 to 65535, excluding 7443, and configure that exact port in Cloudflare.")
  if (!draft.routesReviewed) throw new Error("Review the dedicated tunnel's dashboard routes before connecting it.")
  return { hostPort: Number(draft.localPort), options: { hostname, token: cloudflareTokenInput(draft.token).trim() || undefined, remember: draft.remember, routesReviewed: true } }
}
