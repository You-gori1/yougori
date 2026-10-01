import { useEffect, useRef, useState } from "react"
import { releaseApi, type ReleaseStatus } from "@/api/release-api"
import { Button } from "@/components/ui/button"

const CHECK_INTERVAL = 12 * 60 * 60 * 1000
const RETRY_INTERVAL = 15 * 60 * 1000
const REMIND_INTERVAL = 24 * 60 * 60 * 1000

function Offer({ status, onLater }: { status: ReleaseStatus; onLater?(): void }) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  return <div className="flex flex-wrap items-center justify-between gap-3">
    <div className="min-w-0">
      <p className="text-sm font-medium">Yougori {status.latest} is available</p>
      <p className="mt-1 text-xs text-muted-foreground">Installed: {status.current}. {status.channel === "preview" ? "Preview release. Unsigned installation requires your consent." : "An update is ready to download."}</p>
      <p className="mt-1 text-xs text-muted-foreground">Get the installer and update when you’re ready.</p>
      {error && <p role="alert" className="mt-1 text-xs text-destructive-foreground">{error}</p>}
    </div>
    <div className="flex shrink-0 gap-2">
      {onLater && <Button size="sm" variant="ghost" onClick={onLater}>Later</Button>}
      <Button size="sm" disabled={busy} loading={busy} onClick={() => {
        setBusy(true)
        setError("")
        void releaseApi.openDownloads().catch(reason => setError(String(reason))).finally(() => setBusy(false))
      }}>Get update</Button>
    </div>
  </div>
}

export function ReleaseUpdateNotice() {
  const [status, setStatus] = useState<ReleaseStatus | null>(null)
  const initial = useRef<Promise<ReleaseStatus | null> | null>(null)
  const deferred = useRef<{ version: string; until: number } | null>(null)
  useEffect(() => {
    let active = true
    let timer: number | undefined
    const show = (value: ReleaseStatus | null) => {
      const reminder = deferred.current
      const postponed = reminder && value?.latest === reminder.version && reminder.until > Date.now()
      if (active) setStatus(value?.updateAvailable && !value.remindLater && !postponed ? value : null)
    }
    const schedule = (delay: number) => {
      if (active) timer = window.setTimeout(() => { void check(releaseApi.check()) }, delay)
    }
    const check = async (request: Promise<ReleaseStatus | null>) => {
      try { show(await request); schedule(CHECK_INTERVAL) }
      catch { schedule(RETRY_INTERVAL) } // Offline checks stay quiet.
    }
    initial.current ??= releaseApi.check()
    void check(initial.current)
    return () => { active = false; window.clearTimeout(timer) }
  }, [])
  if (!status) return null
  return <aside aria-label="Yougori update available" className="mb-5 rounded-lg border bg-card p-4">
    <Offer status={status} onLater={() => {
      deferred.current = { version: status.latest, until: Date.now() + REMIND_INTERVAL }
      setStatus(null)
      void releaseApi.remindLater(status.latest).catch(() => undefined)
    }} />
  </aside>
}

export function ReleaseUpdateSettings() {
  const [status, setStatus] = useState<ReleaseStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState("")
  return <section className="preferences-section" aria-labelledby="preferences-updates-title">
    <div className="preferences-section-label"><h3 id="preferences-updates-title">Updates</h3><p>Checked automatically every 12 hours</p></div>
    <div className="preferences-section-body">
      {status?.updateAvailable ? <Offer status={status} /> : null}
      {message && <p role="status" className="mb-2 text-sm text-muted-foreground">{message}</p>}
      <Button className={status?.updateAvailable ? "mt-4" : undefined} size="sm" variant="outline" disabled={busy} loading={busy} onClick={() => {
        setBusy(true)
        setMessage("")
        void releaseApi.check(true).then(value => {
          setStatus(value)
          setMessage(!value ? "Update checks are available in the installed Yougori app." : value.updateAvailable ? "" : value.downloadAvailable ? `Yougori ${value.current} is up to date.` : "No newer downloadable release is available for this platform.")
        }).catch(reason => setMessage(String(reason))).finally(() => setBusy(false))
      }}>Check for updates</Button>
    </div>
  </section>
}
