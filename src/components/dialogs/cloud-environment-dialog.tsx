import { useRef, useState } from "react"
import { cloudApi, type CloudProfile } from "@/api/cloud-api"
import { usePlatform } from "@/context/platform-context"
import { Button } from "@/components/ui/button"
import { Dialog, DialogPopup, DialogHeader, DialogTitle, DialogDescription, DialogPanel, DialogFooter } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { useCloudProfileDraft } from "@/lib/cloud-profile-draft"

export function CloudEnvironmentDialog({ open, onOpenChange, environmentId, initialProfile, embedded = false, onBusyChange }: { embedded?: boolean; onBusyChange?(busy: boolean): void; open: boolean; onOpenChange(open: boolean): void; environmentId?:string; initialProfile?:Partial<CloudProfile> }) {
  const { addCloudEnvironment,configureCloudEnvironment } = usePlatform()
  const { profile, setProfile, reset } = useCloudProfileDraft(environmentId, initialProfile)
  const [verified, setVerified] = useState("")
  const [busy, setBusy] = useState<"connect" | "save" | "browse" | null>(null)
  const [error, setError] = useState("")
  const guard = useRef(false)
  const signature = JSON.stringify(profile)
  const connected = verified === signature
  const edit = (change: Partial<CloudProfile>) => {
    const next = { ...profile, ...change }
    if (next.host !== profile.host || next.port !== profile.port) next.hostKey = ""
    setProfile(next)
    setError("")
  }
  const perform = async (action: NonNullable<typeof busy>, operation: () => Promise<void>) => {
    if (guard.current) return
    guard.current = true; onBusyChange?.(true); setBusy(action); setError("")
    try { await operation() }
    catch (e) { setError(String(e instanceof Error ? e.message : e)) }
    finally { guard.current = false; setBusy(null); onBusyChange?.(false) }
  }
  const connect = () => {
    void perform("connect", async () => {
      setVerified("")
      const checked = await cloudApi.testConnection(profile, environmentId)
      setProfile({ ...profile, hostKey: checked.hostKey })
      setVerified(JSON.stringify(checked))
    })
  }
  const save = () => {
    if (!connected) return
    void perform("save", async () => {
      if (environmentId) await configureCloudEnvironment(environmentId, profile)
      else await addCloudEnvironment(profile)
      setVerified("")
      reset()
      onOpenChange(false)
    })
  }
  const content = <>
      {embedded ? <p className="px-6 pt-4 text-sm text-muted-foreground">Connect an existing Linux server over SSH. Its power stays under your control.</p> : <DialogHeader><DialogTitle className="text-base">Cloud environment</DialogTitle><DialogDescription>Connect an existing Linux server over SSH. Its power stays under your control.</DialogDescription></DialogHeader>}
      <form className="contents" onSubmit={e => { e.preventDefault(); connect() }}>
        <DialogPanel className="space-y-5">
          <fieldset disabled={Boolean(busy)} className="space-y-5">
            <div className="flex flex-wrap gap-1.5" role="group" aria-label="Cloud provider">{([['aws', 'AWS EC2'], ['google', 'Google Compute Engine'], ['azure', 'Azure VM'], ['other', 'Other server']] as const).map(([value, label]) => <Button key={value} type="button" size="sm" variant={profile.vendor === value ? "default" : "outline"} aria-pressed={profile.vendor === value} onClick={() => edit({ vendor: value })}>{label}</Button>)}</div>
            <div className="grid gap-4 sm:grid-cols-2">
              <Label className="grid gap-2">Node name<Input autoFocus required value={profile.name} onChange={e => edit({ name: e.target.value })} placeholder="Production database" /></Label>
              <Label className="grid gap-2">Server address<Input required value={profile.host} onChange={e => edit({ host: e.target.value })} placeholder="IP address or hostname" spellCheck={false} /></Label>
              <Label className="grid gap-2">SSH username<Input required value={profile.username} onChange={e => edit({ username: e.target.value })} autoComplete="off" spellCheck={false} /></Label>
              <Label className="grid gap-2">SSH port<Input required type="number" min={1} max={65535} value={profile.port || ""} onChange={e => edit({ port: Number(e.target.value) })} /></Label>
            </div>
            <div className="space-y-2"><Label htmlFor="cloud-identity">SSH identity file or public key</Label><div className="flex gap-2"><Input id="cloud-identity" required value={profile.identityFile} onChange={e => edit({ identityFile: e.target.value })} placeholder="Choose a key file or paste an SSH public key" spellCheck={false} /><Button type="button" variant="outline" onClick={() => void perform("browse", async () => { const path = await cloudApi.selectKey(); if (path) edit({ identityFile: path }) })}>Browse</Button></div><p className="text-xs text-muted-foreground">Pasted public keys use the matching private key in this PC’s SSH agent. Otherwise, browse for your private key file. Requires OpenSSH and Python 3 on the server.</p></div>
          </fieldset>
          <p className="text-xs leading-relaxed text-muted-foreground">Private connections to your local nodes and selected shared data only. Local network and Public access are unavailable for cloud nodes. Nothing is installed as a background service.</p>
          {connected ? <p role="status" className="text-sm text-emerald-600 dark:text-emerald-400">Connection successful</p> : null}
          {error ? <p role="alert" className="text-sm text-destructive whitespace-pre-wrap">{error}</p> : null}
        </DialogPanel>
        <DialogFooter>
          <Button disabled={Boolean(busy)} type="button" variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button type="submit" variant={connected ? "outline" : "default"} disabled={Boolean(busy)} loading={busy === "connect"}>Connect</Button>
          <Button type="button" disabled={Boolean(busy) || !connected} loading={busy === "save"} onClick={save}>{environmentId ? "Save SSH access" : "Add cloud node"}</Button>
        </DialogFooter>
      </form>
    </>
  return embedded ? content : <Dialog open={open} onOpenChange={value => { if (!busy) onOpenChange(value) }}>
    <DialogPopup className="w-[min(760px,calc(100vw-2rem))] max-w-none" closeProps={{ disabled: Boolean(busy) }}>{content}</DialogPopup>
  </Dialog>
}
