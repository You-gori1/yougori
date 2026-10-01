import { run } from "@/api/platform-api"

export interface EnvironmentDownload {
  environmentId: string
  active: boolean
  url: string | null
  domain: string | null
  sizeBytes: number
  downloads: number
}
// In-memory identity: reopening Yougori never restores a public download link.
const ownerId = crypto.randomUUID()
const owned = new Set<string>()
const fixtures = new Map<string, EnvironmentDownload>()
const notify = () => window.dispatchEvent(new Event("yougori-download-links-changed"))

export const environmentDownloadApi = {
  async list() {
    return run<EnvironmentDownload[]>("list_environment_downloads", {}, () => [...fixtures.values()])
  },
  async start(environmentId: string, domain?: string) {
    const link = await run<EnvironmentDownload>("start_environment_download", { request: { environmentId, ownerId, domain: domain || null } }, () => {
      const previous = fixtures.get(environmentId)
      if (previous?.active) throw new Error("This environment already has a download link")
      const link = { environmentId, active: true, url: "https://download-preview.invalid/temporary", domain: domain || null, sizeBytes: 0, downloads: previous?.downloads ?? 0 }
      fixtures.set(environmentId, link)
      return link
    })
    owned.add(environmentId)
    notify()
    return link
  },
  async stop(environmentId: string) {
    await run<void>("stop_environment_download", { environmentId }, () => {
      const previous = fixtures.get(environmentId)
      if (previous) fixtures.set(environmentId, { ...previous, active: false, url: null, domain: null, sizeBytes: 0 })
    })
    owned.delete(environmentId)
    notify()
  },
  async heartbeat() {
    if (!owned.size) return this.list()
    const links = await run<EnvironmentDownload[]>("keep_environment_downloads_alive", { ownerId }, () => [...fixtures.values()])
    for (const id of owned) if (!links.some(link => link.environmentId === id && link.active)) owned.delete(id)
    return links
  },
}
