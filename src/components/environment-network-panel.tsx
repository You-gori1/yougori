import { ConfigurationHelp } from "@/components/configuration-help"
import { useState } from "react"
import { workspaceApi } from "@/api/workspace-api"
import { CloudDeploymentCard } from "@/components/cloud-deployment-card"
import { GraphWorkspaceDialogs } from "@/components/graph-workspace-dialogs"
import { SharingPanel } from "@/components/sharing-panel"
import { useWorkspaceFeatures } from "@/components/use-workspace-features"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { usePlatform } from "@/context/platform-context"
import type { Environment } from "@/types/platform"

/** Everything about this environment's access, ports and publishing. */
export function EnvironmentNetworkPanel({ environment }: { environment: Environment }) {
  const { updateContainerNetwork } = usePlatform()
  const workspace = useWorkspaceFeatures([environment])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const features = workspace.decorated[0]?.workspace
  const cloud = environment.kind === "cloud"
  const perform = async (action: () => Promise<unknown>) => {
    if (busy) return
    setBusy(true); setError("")
    try { await action() } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)) } finally { setBusy(false) }
  }

  return <>
    {error ? <p className="px-1 text-xs text-destructive-foreground" role="alert">{error}</p> : null}
    <CloudDeploymentCard key={environment.id} environmentId={environment.id} />
    <section className="inspector-section configuration-setting-row" aria-label="PC permissions">
      <h3 className="inspector-section-title">PC permissions</h3>
      {features?.shares.length ? features.shares.map(share => <p key={share.id} className="mb-2 break-all text-xs">{share.path}<span className="block text-muted-foreground">{share.readOnly ? "View Only" : "View & Edit"}</span></p>) : <p className="inspector-description">No Access</p>}
      <Button className="mt-2" size="xs" variant="outline" disabled={cloud} onClick={() => workspace.openShares(environment.id)}>Manage selected folders</Button>
    </section>
    <section className="inspector-section configuration-setting-row" aria-label="Network and publishing">
      <h3 className="inspector-section-title">Network<ConfigurationHelp label="Network publishing help">Publish service ports to your local network or through a Cloudflare Tunnel.</ConfigurationHelp></h3>
      <label className="flex items-center gap-2 text-xs"><Checkbox disabled={busy || cloud} checked={environment.networkAccess ?? false} onCheckedChange={value => void perform(() => updateContainerNetwork(environment.id, value === true))} />Internet access</label>
      {features?.services.map(service => <div key={service.port} className="mt-2 flex items-center justify-between gap-1 text-xs"><span>TCP {service.port}</span><Button size="xs" variant="ghost" onClick={() => workspace.openService(environment.id, service.port)}>Publish to</Button></div>)}
      <Button className="mt-2" size="xs" variant="outline" onClick={() => workspace.openService(environment.id, 0)}>Add service port</Button>
      {features?.publications.filter(publication => publication.kind === "local").map(publication => <p key={publication.id} className="mt-2 break-all text-xs">Local Network · {publication.status} · {publication.urls.join(", ")}</p>)}
    </section>
    <section className="inspector-section configuration-setting-row" aria-label="Sharing">
      <h3 className="inspector-section-title">Sharing</h3>
      <SharingPanel environmentId={environment.runtime.startsWith("shared://") ? undefined : environment.id} />
    </section>
    <section className="inspector-section configuration-setting-row" aria-label="Cloudflare status card">
      <h3 className="inspector-section-title">Cloudflare Tunnel<ConfigurationHelp label="Cloudflare setup help">Domain, tunnel token, and saved setups are configured per service port. Saved credentials stay in the system credential store.</ConfigurationHelp></h3>
      {features?.publications.filter(publication => publication.kind === "cloudflare").map(publication => <div key={publication.id} className="mb-2 text-xs">
        <p>{publication.status} · {publication.cloudflareAccount ? "Configured account" : "Quick link"}</p>
        {publication.urls.map(url => <button key={url} className="break-all text-left underline" onClick={() => void perform(() => workspaceApi.openUrl(url))} type="button">{url}</button>)}
      </div>)}
      <Button className="mt-2" size="xs" variant="outline" onClick={() => workspace.openService(environment.id, features?.services[0]?.port ?? 0, "cloudflare")}>Configure publishing</Button>
    </section>
    <GraphWorkspaceDialogs model={workspace} />
  </>
}
