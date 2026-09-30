import type { StorageCleanupResult } from "@/types/platform"

export function storageCleanupDescription(cleanup?: StorageCleanupResult) {
  const parts: string[] = []
  if (cleanup?.reclaimedDiskBytes) parts.push(`${(cleanup.reclaimedDiskBytes / 1_073_741_824).toFixed(2)} GB returned from container disks.`)
  if (cleanup?.reclaimedCacheBytes) parts.push(`${(cleanup.reclaimedCacheBytes / 1_073_741_824).toFixed(2)} GB of unused cached images also removed. Original installers and exported backups were kept.`)
  if (!parts.length) parts.push("No additional disk space was reclaimed.")
  parts.push(...cleanup?.warnings ?? [], ...cleanup?.notes ?? [])
  if (!cleanup?.notes?.length && !cleanup?.reclaimedCacheBytes) parts.push("Original installers, exported backups and images still needed by environments were kept.")
  return parts.join("\n")
}
