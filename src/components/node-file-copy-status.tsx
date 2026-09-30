import { CopyIcon } from "lucide-react"
import type { NodeFileCopy } from "@/components/use-node-file-drop"
import { Spinner } from "@/components/ui/spinner"

export function NodeFileCopyStatus({ copy }: { copy?: NodeFileCopy }) {
  if (!copy) return null
  if (copy.error) return <p className="nodrag mt-2 break-words text-[11px] text-destructive-foreground" role="alert">Needs attention</p>
  if (copy.busy) {
    const progress = copy.progress
    const percent = progress?.totalBytes ? Math.min(100, Math.floor(progress.completedBytes / progress.totalBytes * 100)) : null
    const label = progress?.phase === "scanning" ? "Scanning folder" : progress?.phase === "copying" ? "Copying files" : progress?.phase === "finishing" ? "Finishing copy" : "Preparing copy"
    return <div className="nodrag mt-2 space-y-1 text-[11px] text-muted-foreground" role="status">
      <span className="flex items-center gap-1.5"><Spinner className="size-3" aria-hidden="true" />{label}{percent !== null && progress?.phase !== "finishing" ? ` · ${percent}%` : "…"}</span>
      {percent !== null ? <progress aria-label={label} className="h-1 w-full accent-primary" max={100} value={percent} /> : null}
      {progress?.scannedEntries !== undefined ? <span className="block text-[10px]">{progress.scannedEntries.toLocaleString()} items found</span> : null}
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
