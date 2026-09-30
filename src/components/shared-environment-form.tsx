import { useRef, useState } from "react"
import { remoteAccessApi } from "@/api/remote-access-api"
import { usePlatform } from "@/context/platform-context"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"

export function SharedEnvironmentForm({ environmentId, onClose, onConnected, onBusyChange }: { environmentId?: string; onClose(): void; onConnected?(): void; onBusyChange?(busy: boolean): void }) {
  const { refreshPlatform } = usePlatform()
  const [link, setLink] = useState("")
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const lock = useRef(false)
  return <form className="mx-auto w-full max-w-xl space-y-4" onSubmit={event => {
    event.preventDefault()
    if (lock.current) return
    lock.current = true; setBusy(true); onBusyChange?.(true); setError("")
    void Promise.resolve().then(() => remoteAccessApi.connect(link.trim(), username.trim(), password, environmentId)).then(async () => {
      setPassword("")
      await refreshPlatform()
      onConnected?.()
      onClose()
    }).catch(reason => setError(String(reason))).finally(() => {
      lock.current = false; setBusy(false); onBusyChange?.(false)
    })
  }}>
    <p className="text-sm text-muted-foreground">{environmentId ? "Update this node using the owner's current link and recipient credentials. You can also connect it to a different shared environment." : "Connect using the link and recipient credentials supplied by the owner. The shared environment will appear in Nodes and List."}</p>
    <label className="block space-y-1 text-sm">Link<Input required disabled={busy} type="url" placeholder="https://…/share/share-…" value={link} onChange={event => setLink(event.target.value)} /></label>
    <label className="block space-y-1 text-sm">Username<Input required disabled={busy} autoComplete="username" value={username} onChange={event => setUsername(event.target.value)} /></label>
    <label className="block space-y-1 text-sm">Password<Input required disabled={busy} type="password" autoComplete="current-password" value={password} onChange={event => setPassword(event.target.value)} /></label>
    <p className="text-xs text-muted-foreground">Use Yougori share credentials, never your Windows or Cloudflare password. Workloads stay on the owner's machine.</p>
    {error ? <p role="alert" className="text-sm text-destructive-foreground">{error}</p> : null}
    <div className="flex justify-end gap-2 pt-2"><Button type="button" variant="outline" disabled={busy} onClick={onClose}>Cancel</Button><Button type="submit" loading={busy} disabled={busy}>Connect</Button></div>
  </form>
}
