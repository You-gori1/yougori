import { useEffect, useState } from "react"
import { remoteAccessApi, type RemoteShares } from "@/api/remote-access-api"
import { Button } from "@/components/ui/button"
import { toastManager } from "@/components/ui/toast"

export function RemoteAccessStatus() {
  const [shares, setShares] = useState<RemoteShares | null>(null), [busy, setBusy] = useState(false)
  useEffect(() => { let alive = true; let timer: ReturnType<typeof setTimeout>; const poll = async () => { try { const result = await remoteAccessApi.list(); if (alive) setShares(result) } catch { /* Existing indicator stays visible until a successful status refresh. */ } finally { if (alive) timer = setTimeout(() => void poll(), 3000) } }; void poll(); return () => { alive = false; clearTimeout(timer) } }, [])
  const report = (e: unknown) => toastManager.add({ title: "Remote access", description: String(e), type: "error" })
  const sessions = shares?.grants.reduce((count, grant) => count + grant.connectedUsers, 0) ?? 0
  return shares?.url || sessions ? <div className="flex items-center gap-2 rounded-lg border border-primary/30 px-2 py-1" role="status"><p className="text-xs font-medium">Remote access enabled</p><p className="text-xs text-muted-foreground">{sessions} connected {sessions === 1 ? "session" : "sessions"}</p><Button className="h-auto! whitespace-normal py-1.5! text-[11px]!" size="xs" variant="outline" disabled={busy} onClick={() => { setBusy(true); void remoteAccessApi.stop().then(() => remoteAccessApi.list()).then(setShares).catch(report).finally(() => setBusy(false)) }}>Disconnect Remote Users</Button></div> : null
}
