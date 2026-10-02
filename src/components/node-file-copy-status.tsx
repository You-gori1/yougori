import { CopyIcon } from "lucide-react"
import { useEffect, useState } from "react"
import type { NodeFileCopy } from "@/components/use-node-file-drop"
import { Spinner } from "@/components/ui/spinner"

const labels = { scanning: "Scanning folder", preparing: "Preparing copy", archiving: "Archiving files", connecting: "Connecting to environment", copying: "Copying files", sending: "Sending files", extracting: "Extracting files", verifying: "Verifying copy", finishing: "Finishing copy" }
const bytes = (value: number) => value < 1024 ? `${value.toLocaleString()} B` : value < 1024 * 1024 ? `${(value / 1024).toFixed(1)} KB` : value < 1024 * 1024 * 1024 ? `${(value / (1024 * 1024)).toFixed(1)} MB` : `${(value / (1024 * 1024 * 1024)).toFixed(1)} GB`
const age = (value: number) => value < 60 ? `${value}s` : value < 3600 ? `${Math.floor(value / 60)}m` : `${Math.floor(value / 3600)}h`

export function NodeFileCopyStatus({ copy }: { copy?: NodeFileCopy }) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!copy?.busy) return
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [copy?.busy])
  if (!copy) return null
  if (copy.error) return <div className="nodrag mt-2 space-y-1 break-words text-[11px] text-destructive-foreground" role="alert"><strong>{copy.cancelled ? "Copy cancelled" : "Copy needs attention"}</strong><p>{copy.error}</p><p className="text-muted-foreground">Originals stay on your computer. Any files already extracted stay in this transfer’s unique import folder for inspection; this copy is incomplete.</p>{copy.progress?.transferId ? <code className="block select-text break-all">{copy.progress.transferId}</code> : null}</div>
  if (copy.busy) {
    const progress = copy.progress
    const percent = progress?.totalBytes ? Math.min(100, Math.floor(progress.completedBytes / progress.totalBytes * 100)) : null
    const label = copy.cancelling ? "Cancelling copy" : labels[progress?.phase ?? "preparing"]
    const lastChanged = progress?.lastProgressAt ? Date.parse(progress.lastProgressAt) : NaN
    const seconds = Number.isFinite(lastChanged) ? Math.max(0, Math.floor((now - lastChanged) / 1000)) : null
    return <div className="nodrag mt-2 space-y-1 text-[11px] text-muted-foreground" role="status">
      <span className="flex items-center gap-1.5"><Spinner className="size-3" aria-hidden="true" />{label}{percent !== null && progress?.phase !== "finishing" ? ` · ${percent}%` : "…"}</span>
      {percent !== null ? <progress aria-label={label} className="h-1 w-full accent-primary" max={100} value={percent} /> : null}
      {progress?.scannedEntries !== undefined ? <span className="block text-[10px]">{progress.scannedEntries.toLocaleString()} items found</span> : null}
      {progress?.totalBytes ? <span className="block text-[10px]">{bytes(progress.completedBytes)} of {bytes(progress.totalBytes)} in this phase</span> : null}
      {progress?.sentBytes !== undefined ? <span className="block text-[10px]">Sent {bytes(progress.sentBytes)} · Confirmed by environment {bytes(progress.confirmedBytes ?? 0)}</span> : null}
      {seconds !== null ? <span className="block text-[10px]">Last progress {age(seconds)} ago</span> : null}
      {progress?.waitingFor ? <span className="block break-all text-[10px]">Waiting for {progress.waitingFor}</span> : null}
      {copy.onCancel ? <button type="button" className="nodrag rounded border px-2 py-1 text-[11px] text-foreground disabled:opacity-50" disabled={copy.cancelling} onClick={event => { event.stopPropagation(); copy.onCancel?.() }}>{copy.cancelling ? "Cancelling…" : "Cancel copy"}</button> : null}
      {copy.cancelError ? <p role="alert" className="text-destructive-foreground">{copy.cancelError}</p> : null}
      <span className="block text-[10px]">Originals stay on your computer.</span>
      {copy.names?.length ? <span className="block break-all">{copy.names.join(", ")}</span> : null}
    </div>
  }
  const result = copy.result
  if (!result) return null
  return <div className="nodrag mt-2 space-y-1 text-[11px] text-muted-foreground" role="status">
    <span className="flex items-center gap-1.5"><CopyIcon aria-hidden="true" className="size-3" />{result.delivery === "drive" ? "Copied to Imported files drive" : "Files copied"}</span>
    {copy.names?.length ? <span className="block break-all">{copy.names.join(", ")}</span> : null}
    <code className="block select-text break-all text-[10px] text-foreground">{result.destination}</code>
    {result.delivery === "drive" ? <span className="block text-[10px]">Open the YOUGORI drive inside your VM.</span> : null}
    {result.skippedLinks ? <span className="block text-[10px]">Skipped {result.skippedLinks} symbolic {result.skippedLinks === 1 ? "link" : "links"} or linked folders.</span> : null}
    {result.skippedIgnored ? <span className="block text-[10px]">Left out {result.skippedIgnored} {result.skippedIgnored === 1 ? "item" : "items"} listed in .yougoriignore.</span> : null}
  </div>
}
