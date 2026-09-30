import type { HostMetrics, HostStorageDrive } from "@/types/platform"

export const driveLabel = (path: string) => path.replace(/[\\/]+$/, "") || "/"

export function storageDrives(host: HostMetrics): HostStorageDrive[] {
  return host.storageDrives ?? [{ path: host.storageDrive ?? "", name: "Storage", fileSystem: "", totalGb: host.totalStorageGb, freeGb: Math.max(0, host.totalStorageGb - host.usedStorageGb), readOnly: false, removable: false }]
}

export function availableStorage(host: HostMetrics, selected?: string | null): number {
  if (!selected) return Math.max(0, host.totalStorageGb - host.usedStorageGb)
  const drive = storageDrives(host).find(drive => drive.path === selected)
  if (!drive) throw new Error("The selected storage drive is unavailable. Reconnect it and retry.")
  if (drive.readOnly) throw new Error("The selected storage drive is read-only.")
  return drive.freeGb
}
