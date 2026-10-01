import { useEffect, useState } from "react"
import { environmentDownloadApi, type EnvironmentDownload } from "@/api/environment-download-api"
import { Button } from "@/components/ui/button"
import { toastManager } from "@/components/ui/toast"

// Own the heartbeat at app level, so closing a node's dialog keeps its link on.
export function EnvironmentDownloadStatus() {
  const [links, setLinks] = useState<EnvironmentDownload[]>([])
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    let active = true, timer: ReturnType<typeof setTimeout>, pending = false
    const poll = async () => {
      if (pending) return
      pending = true
      clearTimeout(timer)
      try { const result = await environmentDownloadApi.heartbeat(); if (active) setLinks(result.filter(link => link.active)) }
      catch { /* An offline engine expires leases itself; do not announce success. */ }
      finally { pending = false; if (active) timer = setTimeout(() => void poll(), 20_000) }
    }
    const changed = () => { void poll() }
    void poll()
    window.addEventListener("yougori-download-links-changed", changed)
    return () => { active = false; clearTimeout(timer); window.removeEventListener("yougori-download-links-changed", changed) }
  }, [])
  if (!links.length) return null
  return <div className="flex items-center gap-2 rounded-lg border border-primary/30 px-2 py-1" role="status">
    <p className="text-xs font-medium">{links.length} {links.length === 1 ? "download link" : "download links"} on</p>
    <Button size="xs" variant="outline" disabled={busy} onClick={() => {
      setBusy(true)
      void Promise.all(links.map(link => environmentDownloadApi.stop(link.environmentId)))
        .then(() => setLinks([])).catch(reason => toastManager.add({ title: "Download links", description: String(reason), type: "error" }))
        .finally(() => setBusy(false))
    }}>Turn off</Button>
  </div>
}
