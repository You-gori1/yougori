import { useRef, useState } from "react"
import { RotateCcwIcon } from "lucide-react"
import { AlertDialog, AlertDialogClose, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogPopup, AlertDialogTitle, AlertDialogTrigger } from "@/components/ui/alert-dialog"
import { Button } from "@/components/ui/button"
import { usePlatform } from "@/context/platform-context"
import type { Environment } from "@/types/platform"

export function FactoryResetDialog({ environment, disabled = false }: { environment: Environment; disabled?: boolean }) {
  const { factoryResetEnvironment } = usePlatform()
  const [open, setOpen] = useState(false)
  const [confirmation, setConfirmation] = useState("")
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState("")
  const lock = useRef(false)
  const stopped = environment.status === "stopped" || environment.status === "error"
  const installer = environment.kind === "fullVm" && /\.iso$/i.test(environment.runtime)
  const reset = async () => {
    if (lock.current || confirmation !== environment.name) return
    lock.current = true; setLoading(true); setError("")
    try { await factoryResetEnvironment(environment.id, confirmation); setOpen(false) }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)) }
    finally { lock.current = false; setLoading(false) }
  }
  return <section aria-label="Factory reset" className="inspector-section">
    <div className="inspector-section-heading">
      <h3 className="inspector-section-title"><RotateCcwIcon aria-hidden="true" />Factory reset</h3>
      <AlertDialog open={open} onOpenChange={value => {
        if (loading) return
        setOpen(value); setConfirmation(""); setError("")
      }}>
        <AlertDialogTrigger render={<Button disabled={disabled || !stopped} size="sm" type="button" variant="destructive-outline" />}>Factory reset</AlertDialogTrigger>
        <AlertDialogPopup>
          <AlertDialogHeader>
            <AlertDialogTitle>Factory reset {environment.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              This permanently deletes this environment’s files, installed apps, sign-ins and local snapshots. Its name, resource settings and original image are kept. Other environments, shared PC folders and external backups are not erased.
              {installer ? " This VM will return to a blank disk and its installer. You will need to install the operating system again." : " It will return to the contents of its original base image."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="flex flex-col gap-2 px-6 pb-4">
            <label className="text-sm" htmlFor={`reset-${environment.id}`}>Type <strong>{environment.name}</strong> to confirm</label>
            <input autoComplete="off" className="h-9 rounded-md border bg-transparent px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring" disabled={loading} id={`reset-${environment.id}`} onChange={event => setConfirmation(event.target.value)} value={confirmation} />
            {error ? <p className="break-words text-sm text-destructive-foreground" role="alert">{error}</p> : null}
            {loading ? <p className="text-sm text-muted-foreground" role="status">Preparing the original image and removing old data…</p> : null}
          </div>
          <AlertDialogFooter>
            <AlertDialogClose render={<Button disabled={loading} type="button" variant="ghost" />}>Cancel</AlertDialogClose>
            <Button disabled={confirmation !== environment.name} loading={loading} onClick={() => void reset()} type="button" variant="destructive">Erase data and reset</Button>
          </AlertDialogFooter>
        </AlertDialogPopup>
      </AlertDialog>
    </div>
    <p className="inspector-description">Start again from the original image. Back up anything you want to keep first.{!stopped ? " Shut down the environment to enable reset." : ""}</p>
  </section>
}
