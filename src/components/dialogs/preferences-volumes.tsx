import { useCallback, useEffect, useState } from "react"
import { volumesApi, volumeSize, volumeUsers, type VolumeListing } from "@/api/volumes-api"
import { Button } from "@/components/ui/button"

/** Named volumes kept by container runtimes, including ones left by deleted environments. */
export function PreferencesVolumes({ open }: { open: boolean }) {
  const [listing, setListing] = useState<VolumeListing | null>(null)
  const [scanned, setScanned] = useState(false)
  const [busy, setBusy] = useState<"" | "scan" | string>("")
  const [confirm, setConfirm] = useState("")
  const [error, setError] = useState("")

  const load = useCallback(async (scan: boolean) => {
    setError("")
    try {
      setListing(await volumesApi.list(scan, scan))
      if (scan) setScanned(true)
    } catch (reason) { setError(String(reason)) }
  }, [])

  useEffect(() => {
    if (!open) return
    setScanned(false)
    setConfirm("")
    void load(false)
  }, [open, load])

  const scan = async () => {
    setBusy("scan")
    await load(true)
    setBusy("")
  }

  const remove = async (name: string) => {
    setBusy(name)
    setError("")
    try {
      await volumesApi.remove(name)
      setConfirm("")
      await load(scanned)
    } catch (reason) { setError(String(reason)) }
    setBusy("")
  }

  const volumes = listing?.volumes ?? []
  return <section className="preferences-section" aria-labelledby="preferences-volumes-title">
    <div className="preferences-section-label">
      <h3 id="preferences-volumes-title">Volumes</h3>
      <p>Named data kept by containers</p>
    </div>
    <div className="preferences-section-body">
      {listing === null && !error ? <p className="preferences-empty">Loading…</p>
        : volumes.length === 0 ? <p className="preferences-empty">{scanned ? "No volumes. They appear when an environment mounts one." : "No volumes in running containers. Check everything to include stopped ones."}</p>
        : <ul className="preferences-volume-list">
          {volumes.map(volume => {
            const users = volumeUsers(volume)
            const size = volumeSize(volume)
            const removing = busy === volume.name
            return <li key={volume.name} className="preferences-volume">
              <div className="preferences-volume-main">
                <span className="preferences-volume-name" title={volume.name}>{volume.name}</span>
                <span className="preferences-volume-detail">
                  {volume.inUse ? `Used by ${users.join(", ")}` : "Unused"}{size ? ` · ${size}` : ""}
                </span>
              </div>
              {!volume.inUse && volume.stored.length > 0 && (confirm === volume.name
                ? <div className="preferences-volume-confirm">
                  <span>Delete it and all its files?</span>
                  <Button disabled={!!busy} variant="ghost" size="sm" onClick={() => setConfirm("")}>Cancel</Button>
                  <Button disabled={!!busy} loading={removing} variant="destructive" size="sm" onClick={() => void remove(volume.name)}>Delete</Button>
                </div>
                : <Button disabled={!!busy} variant="outline" size="sm" onClick={() => setConfirm(volume.name)}>Remove</Button>)}
            </li>
          })}
        </ul>}
      <div className="preferences-volume-actions">
        <p className="preferences-hint">{scanned ? listing?.note : "Shows running containers only. Checking everything may start stopped runtimes."}</p>
        <Button disabled={!!busy} loading={busy === "scan"} variant="outline" size="sm" onClick={() => void scan()}>{scanned ? "Check again" : "Check everything"}</Button>
      </div>
      {listing?.problems.map(problem => <p key={problem} className="preferences-hint">Not checked: {problem}</p>)}
      {error && <p role="alert" className="preferences-error preferences-volume-error">{error}</p>}
    </div>
  </section>
}
