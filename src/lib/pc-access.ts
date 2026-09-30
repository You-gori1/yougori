import type { HostShare } from "@/api/workspace-api"

// Every environment, including the isolated CLI, reaches the PC only through
// selected folders. These are the plain-English names used across the UI.
export function pcAccessLevel(shares: HostShare[]) {
  return shares.length ? shares.some(share => !share.readOnly) ? "View & Edit" : "View Only" : "No Access"
}
