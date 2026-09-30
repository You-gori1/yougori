import { run } from "./platform-api"

export interface VolumeMount { environmentId: string; environment: string; target: string; readOnly: boolean; running: boolean }
export interface VolumeCopy { location: string; runtime: "containers" | "gpu"; usedBy: string[]; sizeBytes?: number | null; sizeComplete?: boolean | null }
export interface NamedVolume { name: string; mounts: VolumeMount[]; stored: VolumeCopy[]; inUse: boolean }
export interface VolumeListing { volumes: NamedVolume[]; checked: string[]; problems: string[]; note: string }

const native = (): never => { throw new Error("Volumes require Yougori Desktop") }

export const volumesApi = {
  list: (scan = false, size = false) => run<VolumeListing>("list_volumes", { scan, size }, () => ({ volumes: [], checked: [], problems: [], note: "Browser preview — no container runtime." })),
  remove: (name: string) => run<{ removed: string; from: string[] }>("remove_volume", { name }, native),
}

export function volumeSize(volume: NamedVolume) {
  const sizes = volume.stored.map(copy => copy.sizeBytes).filter((size): size is number => typeof size === "number")
  if (!sizes.length) return null
  const total = sizes.reduce((sum, size) => sum + size, 0)
  const partial = volume.stored.some(copy => copy.sizeComplete === false)
  const units = ["B", "KB", "MB", "GB", "TB"]
  const exponent = total < 1 ? 0 : Math.min(units.length - 1, Math.floor(Math.log(total) / Math.log(1024)))
  const text = exponent === 0 ? `${total} B` : `${(total / 1024 ** exponent).toFixed(1)} ${units[exponent]}`
  return partial ? `${text}+` : text
}

export function volumeUsers(volume: NamedVolume) {
  const users = volume.mounts.map(mount => mount.environment)
  for (const copy of volume.stored) for (const user of copy.usedBy) if (!users.includes(user)) users.push(user)
  return users
}
