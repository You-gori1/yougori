import { useEffect, useState } from "react"
import { HardDriveIcon } from "lucide-react"
import { fileImportApi, type ImportedDrive } from "@/api/file-import-api"
import { Button } from "@/components/ui/button"
import type { Environment } from "@/types/platform"

export function ImportedFilesDrives({ environment }: { environment: Environment }) {
  const [drives, setDrives] = useState<ImportedDrive[]>([])
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    let disposed = false
    void fileImportApi.drives(environment.id).then(drives => { if (!disposed) setDrives(drives) }).catch((reason: unknown) => { if (!disposed) setError(String(reason)) })
    return () => { disposed = true }
  }, [environment.id, environment.status])
  const change = async (drive: ImportedDrive) => {
    setBusy(true); setError("")
    try { setDrives(await fileImportApi.setDriveAttached(environment.id, drive.id, !drive.attached)) }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)) }
    finally { setBusy(false) }
  }
  return <section className="inspector-section" aria-label="Imported files">
    <h3 className="inspector-section-title"><HardDriveIcon aria-hidden="true" />Imported files</h3>
    <p className="inspector-description">Drop files or folders onto this VM’s node to copy them to a separate YOUGORI drive. Originals stay on your computer.</p>
    {drives.length ? <>
      <p className="inspector-description">Shut down the VM to connect or disconnect these drives. Disconnected copies stay saved. Copy important files to the VM’s main disk to include them in its snapshots and backups.</p>
      {drives.map(drive => <div className="mt-2 flex items-center justify-between gap-3 text-xs" key={drive.id}>
        <span>YOUGORI · {drive.id.slice(0, 8)}<span className="ml-2 text-muted-foreground">{drive.attached ? "Connected" : "Saved"}</span></span>
        <Button disabled={busy || environment.status !== "stopped"} onClick={() => void change(drive)} size="xs" variant="outline">{drive.attached ? "Disconnect drive" : "Connect drive"}</Button>
      </div>)}
    </> : null}
    {error ? <p className="mt-2 text-xs text-destructive-foreground" role="alert">{error}</p> : null}
  </section>
}
