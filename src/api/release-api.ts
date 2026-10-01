import { invoke, isTauri } from "@tauri-apps/api/core"

export interface ReleaseStatus {
  current: string
  latest: string
  channel: string
  platform: string
  updateAvailable: boolean
  downloadAvailable: boolean
  notes: string
  remindLater: boolean
}

export const releaseApi = {
  check(force = false): Promise<ReleaseStatus | null> {
    // Browser previews and guest windows do not check the host's release feed.
    return isTauri() ? invoke("check_release_update", { force }) : Promise.resolve(null)
  },
  remindLater(version: string): Promise<void> {
    return invoke("remind_release_update_later", { version })
  },
  openDownloads(): Promise<void> {
    return invoke("open_release_downloads")
  },
}
