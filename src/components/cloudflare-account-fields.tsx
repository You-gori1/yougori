import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react"
import { workspaceApi, type SavedCloudflareAccount } from "@/api/workspace-api"
import { savedCloudflareDraft, type CloudflareDraft } from "@/lib/cloudflare-account"
import { cloudflareTokenInput } from "@/lib/cloudflare-token"
import type { PublicAccessPreset } from "@/lib/public-access-presets"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Radio, RadioGroup } from "@/components/ui/radio-group"

export function CloudflareAccountFields({ environmentId, port, value, onChange, onLoadingChange, busy, perform, refreshKey, compact = false, presets = [], selectedPresetId = null, onSelectPreset, anyPresetPort = false }: {
  environmentId: string; port: number; value: CloudflareDraft; onChange: Dispatch<SetStateAction<CloudflareDraft>>; onLoadingChange(loading: boolean): void; busy: boolean; perform(action: () => Promise<unknown>): Promise<void>; refreshKey: string; compact?: boolean; presets?: PublicAccessPreset[]; selectedPresetId?: string | null; onSelectPreset?(id: string | null): void
  /** Saved domains made for another app port can be chosen; the caller passes presetPort when connecting. */
  anyPresetPort?: boolean
}) {
  const [saved, setSaved] = useState<SavedCloudflareAccount | null>(null)
  const [loadError, setLoadError] = useState("")
  const [revision, setRevision] = useState(0)
  const restored = useRef(false)
  useEffect(() => {
    let disposed = false
    onLoadingChange(true)
    void workspaceApi.savedCloudflare(environmentId, port).then(result => {
      if (disposed) return
      const draft = savedCloudflareDraft(result)
      setSaved(result); setLoadError("")
      if (!restored.current) {
        restored.current = true
        if (draft) onChange(current => current.hostname || current.localPort || current.token ? current : draft)
      }
    }).catch(() => { if (!disposed) setLoadError("Could not read saved credentials. You can paste a token for this session or retry.") })
      .finally(() => { if (!disposed) onLoadingChange(false) })
    return () => { disposed = true }
  }, [environmentId, port, revision, onChange, onLoadingChange, refreshKey])
  const origin = /^\d+$/.test(value.localPort) && Number(value.localPort) > 0 && Number(value.localPort) <= 65535 ? `http://127.0.0.1:${Number(value.localPort)}` : "http://127.0.0.1:<local tunnel port>"
  return <section aria-label="Cloudflare account options" className={compact ? "space-y-3" : "flex flex-col gap-3 rounded-lg border p-3"}>
    <RadioGroup aria-label="Cloudflare account mode" className={compact ? "grid grid-cols-1 gap-2 sm:grid-cols-2" : "flex flex-col gap-2"} disabled={busy} value={selectedPresetId ? `preset:${selectedPresetId}` : value.mode} onValueChange={mode => {
      if (mode.startsWith("preset:")) { onSelectPreset?.(mode.slice(7)); return }
      onSelectPreset?.(null)
      onChange(current => ({ ...current, mode: mode === "account" ? "account" : "quick", token: "", routesReviewed: false }))
    }}>
      <Label className={compact ? `flex min-h-16 cursor-pointer items-start gap-3 rounded-xl border p-3 text-sm ${!selectedPresetId && value.mode === "quick" ? "border-primary bg-primary/5" : "bg-card hover:border-primary/40"}` : "flex items-center gap-2 text-sm"}><Radio aria-label="Quick link — no account" aria-description="A temporary link, ready in one step" value="quick" /><span aria-hidden="true"><span className="block font-medium">Quick link — no account</span>{compact ? <span className="mt-1 block text-xs text-muted-foreground">A temporary link, ready in one step</span> : null}</span></Label>
      <Label className={compact ? `flex min-h-16 cursor-pointer items-start gap-3 rounded-xl border p-3 text-sm ${!selectedPresetId && value.mode === "account" ? "border-primary bg-primary/5" : "bg-card hover:border-primary/40"}` : "flex items-center gap-2 text-sm"}><Radio aria-label="Use my Cloudflare account (optional)" aria-description="Your own domain and tunnel" value="account" /><span aria-hidden="true"><span className="block font-medium">Use my Cloudflare account (optional)</span>{compact ? <span className="mt-1 block text-xs text-muted-foreground">Your own domain and tunnel</span> : null}</span></Label>
      {compact && presets.length ? <div className="col-span-full space-y-2 pt-1"><p className="text-xs font-medium text-muted-foreground">{anyPresetPort ? "Saved domains" : `Saved domains · choose one for app port :${port}`}</p><div className="grid max-h-40 gap-2 overflow-y-auto pr-1 sm:grid-cols-2">{presets.map(preset => { const otherPort = !anyPresetPort && preset.port !== port; return <Label key={preset.id} className={`flex min-w-0 cursor-pointer items-start gap-2 rounded-lg border p-2 text-xs ${selectedPresetId === preset.id ? "border-primary bg-primary/5" : "bg-card hover:border-primary/40"} ${otherPort ? "cursor-not-allowed opacity-50" : ""}`}><Radio aria-label={anyPresetPort ? `Use ${preset.hostname}` : `Use ${preset.hostname} for app port ${preset.port}`} disabled={otherPort} value={`preset:${preset.id}`} /><span aria-hidden="true" className="min-w-0"><strong className="block truncate">{preset.hostname}</strong><span className="block text-muted-foreground">{anyPresetPort ? `Tunnel :${preset.hostPort}` : `App :${preset.port} · tunnel :${preset.hostPort}`}{otherPort ? " · different port" : ""}</span></span></Label> })}</div></div> : null}
    </RadioGroup>
    {selectedPresetId ? <p className="text-xs text-muted-foreground">Uses the securely saved tunnel token. A saved domain can serve one environment at a time.</p> : value.mode === "account" ? <>
      <p className="text-xs text-muted-foreground">Works with free and paid Cloudflare accounts. Sign in on Cloudflare, then authenticate this connector using a dedicated tunnel token. Yougori never needs your Cloudflare password.</p>
      <div className="flex flex-wrap gap-2"><Button type="button" size="sm" variant="outline" disabled={busy} onClick={() => void perform(() => workspaceApi.openUrl("https://dash.cloudflare.com/"))}>Cloudflare login / dashboard</Button><Button type="button" size="sm" variant="link" disabled={busy} onClick={() => void perform(() => workspaceApi.openUrl("https://developers.cloudflare.com/tunnel/advanced/tunnel-tokens/"))}>Where to get the token</Button></div>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field><FieldLabel>Public hostname</FieldLabel><Input type="text" placeholder="app.example.com" autoComplete="off" spellCheck={false} maxLength={253} disabled={busy} value={value.hostname} onChange={event => onChange(current => ({ ...current, hostname: event.target.value, routesReviewed: false }))} /><FieldDescription>Use a domain you manage in Cloudflare.</FieldDescription></Field>
        <Field><FieldLabel>Local tunnel port</FieldLabel><Input type="text" inputMode="numeric" placeholder="45000" maxLength={5} disabled={busy} value={value.localPort} onChange={event => onChange(current => ({ ...current, localPort: event.target.value, routesReviewed: false }))} /><FieldDescription>Choose an unused port on this PC.</FieldDescription></Field>
      </div>
      <div className="rounded-lg bg-muted/50 p-3 text-xs text-muted-foreground"><p>Set this HTTP service URL in Cloudflare → Networking → Tunnels:</p><code className="mt-1 block break-all font-medium text-foreground" aria-label="Cloudflare service URL">{origin}</code><p className="mt-1">Paste the tunnel token or install command below. Yougori runs the connector for you.</p></div>
      <Field><FieldLabel>Tunnel token</FieldLabel><Input type="password" autoComplete="new-password" spellCheck={false} maxLength={4096} placeholder={saved?.saved ? "Saved securely — leave empty to reuse" : "Paste token or Cloudflare command"} disabled={busy} value={value.token} onChange={event => onChange(current => ({ ...current, token: cloudflareTokenInput(event.target.value), routesReviewed: false }))} /><FieldDescription>{saved?.saved ? "A saved token is available for this environment and port. It is never sent back to this form." : "Leave Remember unchecked to use the token for this connection only."}</FieldDescription></Field>
      <Label className="flex items-center gap-2 text-xs"><Checkbox disabled={busy} checked={value.remember} onCheckedChange={checked => onChange(current => ({ ...current, remember: checked === true }))} />Remember for this node and port</Label>
      <p className="text-xs text-muted-foreground">Saves the hostname, local tunnel port, and token securely on this PC after connecting. Next time you connect this port to Public access, Yougori reuses them without opening setup.</p>
      {saved?.saved ? <div className="space-y-1"><Button type="button" size="sm" variant="ghost" className="self-start" disabled={busy} onClick={() => void perform(async () => { await workspaceApi.forgetCloudflare(environmentId, port); setSaved({ saved: false, hostname: "", hostPort: null }); onChange(current => ({ ...current, token: "", remember: false, routesReviewed: false })) })}>{compact ? "Forget token for this node" : "Forget saved token"}</Button>{compact && presets.some(preset => preset.port === port && preset.hostname === value.hostname) ? <p className="text-xs text-muted-foreground">The reusable Saved setup has its own copy. Remove it from Saved setups to forget that copy too.</p> : null}</div> : null}
      {loadError ? <div className="flex flex-col gap-1"><p role="alert" className="text-xs text-destructive-foreground">{loadError}</p><Button type="button" size="xs" variant="ghost" disabled={busy} onClick={() => setRevision(current => current + 1)}>Retry credential lookup</Button></div> : null}
      <p className="text-xs text-muted-foreground">Use a dedicated tunnel with only this service’s route. Other dashboard routes could expose other host services; Yougori does not verify or edit those routes. Do not run replicas pointing to different services.</p>
      <Label className="flex items-start gap-2 text-xs"><Checkbox disabled={busy} checked={value.routesReviewed} onCheckedChange={checked => onChange(current => ({ ...current, routesReviewed: checked === true }))} />I reviewed this dedicated tunnel’s routes</Label>
      <p className="text-xs text-muted-foreground">Account authentication does not make visitors log in. Configure Cloudflare Access in your dashboard if you want visitor login. Disconnect stops this connector; forgetting a saved token does not stop an active connector or revoke it in Cloudflare.</p>
    </> : <p className="text-xs text-muted-foreground">No account or setup needed. This link changes when the tunnel restarts.</p>}
  </section>
}
