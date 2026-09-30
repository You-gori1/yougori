import { useCallback, useEffect, useRef, useState } from "react"
import { money, runpodApi, RUNPOD_CONSOLE, STORAGE_RATES, type EndpointRequest, type PodRequest, type RunpodAccount, type RunpodCatalog, type RunpodResources, type RunpodStatus } from "@/api/runpod-api"
import { usePlatform } from "@/context/platform-context"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { toastManager } from "@/components/ui/toast"
import { validName } from "@/lib/runpod"
import { EndpointWorkbench } from "./neocloud-endpoint"
import { AccountPanel, OrderFrame, type Order } from "./neocloud-frame"
import { PodWorkbench } from "./neocloud-pod"
import { Badge, Field, Heading, Tile, WebLink, Workbench } from "./neocloud-ui"

type Product = "gpu" | "cpu" | "serverless" | "volume"

/** A dashboard newer than the running app finds no Neocloud commands; say what fixes it. */
function explain(error: unknown): string {
  const text = String(error instanceof Error ? error.message : error)
  return /Command runpod_\w+ not found/.test(text)
    ? "The running Yougori is older than this screen. Quit Yougori from the tray and start it again, then open Neocloud."
    : text
}

const products: { id: Product; title: string; text: string; points: string[] }[] = [
  { id: "gpu", title: "GPU pod", text: "A GPU machine that opens right here.", points: ["Terminal, files and Jupyter", "From entry GPUs to H100 and B200", "Billed per second while it runs"] },
  { id: "serverless", title: "Serverless endpoint", text: "Turn a model into an API.", points: ["Chat, images, speech, video", "Scales to zero when idle", "Ready-made models, one form"] },
  { id: "cpu", title: "CPU pod", text: "A Linux machine without a GPU.", points: ["Scripts, servers and data work", "Opens like any environment", "Cheaper than a GPU"] },
  { id: "volume", title: "Network volume", text: "Storage that outlives pods.", points: ["Keep models and datasets once", "Attach to any pod or endpoint", `About ${money(100 * STORAGE_RATES.network)}/month per 100 GB`] },
]

const elsewhere = [
  { title: "Instant Clusters", text: "Several GPU machines on a fast network, for large training jobs.", url: "https://docs.runpod.io/instant-clusters" },
  { title: "Public Endpoints", text: "Ready model APIs such as Flux and Whisper, paid per request.", url: "https://docs.runpod.io/public-endpoints/overview" },
  { title: "Savings plans", text: "Pay upfront for a GPU and save on long-running pods.", url: `${RUNPOD_CONSOLE}/user/billing` },
]

export function NeocloudForm({ onClose, onBusyChange }: { onClose(): void; onBusyChange?(busy: boolean): void }) {
  const { state, refreshPlatform } = usePlatform()
  const [status, setStatus] = useState<RunpodStatus | null>(null)
  const [catalog, setCatalog] = useState<RunpodCatalog | null>(null)
  const [product, setProduct] = useState<Product | null>(null)
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)
  const alive = useRef(true)
  const taken = state?.environments.map(e => e.name) ?? []

  const load = useCallback(async () => {
    setError("")
    try {
      let next = await runpodApi.status()
      // The tools install by themselves; only the API key needs the user.
      if (alive.current && !next.installed) { setStatus(next); next = await runpodApi.connect() }
      if (!alive.current) return
      setStatus(next)
      if (next.connected) {
        const result = await runpodApi.catalog()
        if (alive.current) setCatalog(result)
      }
    } catch (e) { if (alive.current) { setError(explain(e)); setStatus(s => s ?? { installed: true, connected: false }) } }
  }, [])
  useEffect(() => { alive.current = true; void load(); return () => { alive.current = false } }, [load])
  useEffect(() => { onBusyChange?.(busy) }, [busy, onBusyChange])

  const run = async (work: () => Promise<unknown>, done: string, detail: string) => {
    if (busy) return
    setBusy(true); setError("")
    try {
      await work()
      await refreshPlatform().catch(() => undefined)
      toastManager.add({ title: done, description: detail, type: "success" })
      onClose()
    } catch (e) { if (alive.current) setError(explain(e)) }
    finally { if (alive.current) setBusy(false) }
  }
  const createPod = (request: PodRequest) => void run(() => runpodApi.createPod(request), `Starting ${request.name}`, "It opens here as soon as it's ready, usually within a few minutes.")
  const createEndpoint = (request: EndpointRequest) => void run(() => runpodApi.createEndpoint(request), `${request.name} is deploying`, "Its API address is on the node, with a box to try it.")
  const createVolume = (name: string, location: string, sizeGb: number) => void run(() => runpodApi.volume("create", { name, location, sizeGb }), `Created ${name}`, "Attach it when you create a pod or endpoint.")

  const account = catalog?.account ?? status?.account
  const back = () => { setProduct(null); setError("") }

  let body
  if (status === null) body = <Workbench main={<Loading text="Checking your Neocloud account…" />} />
  else if (!status.connected) body = <Workbench main={<Connect status={status} error={error} onConnected={next => { setStatus(next); setError(""); void load() }} />} />
  else if (!catalog) body = <Workbench main={<Loading text="Loading GPUs, prices and software…" />} />
  else if (product === "gpu" || product === "cpu") body = <PodWorkbench key={product} compute={product} catalog={catalog} account={account} taken={taken} busy={busy} error={error} onBack={back} onCreate={createPod} />
  else if (product === "serverless") body = <EndpointWorkbench catalog={catalog} account={account} taken={taken} busy={busy} error={error} onBack={back} onCreate={createEndpoint} />
  else if (product === "volume") body = <VolumeWorkbench catalog={catalog} account={account} busy={busy} error={error} onBack={back} onCreate={createVolume} />
  else body = <Home catalog={catalog} account={account} error={error} onChoose={setProduct} onChanged={() => void load()} onAttached={onClose}
    onDisconnect={() => { void runpodApi.disconnect().then(next => { setStatus(next); setCatalog(null) }).catch(e => setError(explain(e))) }} />
  return body
}

function Loading({ text }: { text: string }) {
  return <div className="neo-connect"><p role="status" className="neo-note">{text}</p></div>
}

function Connect({ status, error: loadError, onConnected }: { status: RunpodStatus; error: string; onConnected(status: RunpodStatus): void }) {
  const [key, setKey] = useState("")
  const [working, setWorking] = useState(false)
  const [error, setError] = useState("")
  const connect = async () => {
    if (working || !key.trim()) return
    setWorking(true); setError("")
    const credential = key.trim(); setKey("")
    try {
      const next = await runpodApi.connect(credential)
      if (next.connected) onConnected(next)
      else setError(next.message || "This key was not accepted.")
    } catch (e) { setError(explain(e)) }
    finally { setWorking(false) }
  }
  const shown = error || loadError
  return <div className="neo-connect">
    <Heading kicker="Neocloud" title="Rent GPUs in the cloud">Connect your account once. Then create GPU machines, model APIs and storage from here, and open them like any other environment.</Heading>
    <ol className="neo-numbered">
      <li><span>Open the <WebLink url={`${RUNPOD_CONSOLE}/user/settings`}>API keys page</WebLink> and sign in, or create a free account.</span></li>
      <li><span>Create a key with read and write access and copy it.</span></li>
      <li><span>Paste it below. It's kept in your system's credential store.</span></li>
    </ol>
    <form className="neo-row" style={{ alignItems: "flex-end" }} onSubmit={event => { event.preventDefault(); void connect() }}>
      <Field label="API key" className="flex-1"><Input className="neo-input" type="password" autoComplete="off" value={key} onChange={e => setKey(e.target.value)} placeholder="Paste your key" /></Field>
      <Button type="submit" className="neo-cta" disabled={working || !key.trim()} loading={working}>Connect</Button>
    </form>
    {!status.installed && !shown ? <p className="neo-note">Getting Neocloud ready on this PC…</p> : null}
    {status.message && status.installed && !shown ? <p className="neo-note">{status.message}</p> : null}
    {shown ? <p role="alert" className="neo-error">{shown}</p> : null}
  </div>
}

function Home({ catalog, account, error, onChoose, onChanged, onAttached, onDisconnect }: { catalog: RunpodCatalog; account: RunpodAccount | null | undefined; error: string; onChoose(product: Product): void; onChanged(): void; onAttached(): void; onDisconnect(): void }) {
  const inStock = catalog.gpus.filter(g => g.available && g.securePrice != null)
  const cheapest = [...inStock].sort((a, b) => a.securePrice! - b.securePrice!)[0]
  const biggest = [...inStock].sort((a, b) => (b.vramGb ?? 0) - (a.vramGb ?? 0))[0]
  return <Workbench
    main={<>
      <Heading kicker="Neocloud" title="What would you like to create?">GPUs, model APIs and storage in the cloud, opened and managed right here.</Heading>
      <div className="neo-section">
        <div className="neo-grid is-wide">
          {products.map(p => <Tile key={p.id} hero onSelect={() => onChoose(p.id)} title={p.title} label={`Create a ${p.title}`}
            badge={p.id === "gpu" && cheapest ? <Badge tone="primary">from {money(cheapest.securePrice)}/hr</Badge> : p.id === "serverless" ? <Badge tone="good">$0 when idle</Badge> : null}
            meta={p.id === "gpu" && inStock.length ? `${inStock.length} GPU types in stock now${biggest ? `, up to ${biggest.vramGb} GB` : ""}` : undefined}>
            {p.text}
            <ul className="neo-tile-list">{p.points.map(point => <li key={point}>{point}</li>)}</ul>
          </Tile>)}
        </div>
      </div>
      <div className="neo-section">
        <p className="neo-h3">Also available in the console</p>
        <div className="neo-grid">{elsewhere.map(e => <div key={e.title} className="neo-panel"><p className="neo-h2">{e.title}</p><p className="neo-note">{e.text}</p><WebLink url={e.url}>Open</WebLink></div>)}</div>
      </div>
    </>}
    side={<>
      {error ? <p role="alert" className="neo-panel neo-error">{error}</p> : null}
      <AccountPanel account={account} />
      {catalog.issues.length ? <p className="neo-note">Some details did not load: {catalog.issues.join(" · ")}</p> : null}
      <YourCloud catalog={catalog} onChanged={onChanged} onAttached={onAttached} />
      <p className="neo-links">
        <WebLink url={RUNPOD_CONSOLE}>Console</WebLink>
        <WebLink url="https://www.runpod.io/pricing">Pricing</WebLink>
        <WebLink url="https://docs.runpod.io">Documentation</WebLink>
        <button type="button" className="neo-link" onClick={onDisconnect}>Disconnect account</button>
      </p>
    </>}
  />
}

function YourCloud({ catalog, onChanged, onAttached }: { catalog: RunpodCatalog; onChanged(): void; onAttached(): void }) {
  const { refreshPlatform } = usePlatform()
  const [resources, setResources] = useState<RunpodResources | null>(null)
  const [working, setWorking] = useState("")
  const [error, setError] = useState("")
  const [confirm, setConfirm] = useState("")
  const [grow, setGrow] = useState<{ id: string; size: number } | null>(null)
  const [registry, setRegistry] = useState<{ name: string; username: string; password: string } | null>(null)
  useEffect(() => { let alive = true; void runpodApi.resources().then(r => { if (alive) setResources(r) }).catch(() => undefined); return () => { alive = false } }, [])
  const act = async (id: string, work: () => Promise<unknown>, after: () => void = onChanged) => {
    if (working) return
    setWorking(id); setError("")
    try { await work(); after() } catch (e) { setError(explain(e)) } finally { setWorking("") }
  }
  const outside = [
    ...(resources?.pods ?? []).filter(p => !p.attached && p.status !== "TERMINATED").map(p => ({ kind: "pod" as const, id: p.id, name: p.name, text: `${p.gpu ? `${p.gpuCount} × ${p.gpu}` : "CPU"} · ${p.status.toLowerCase()}${p.hourlyUsd ? ` · ${money(p.hourlyUsd)}/hr` : ""}` })),
    ...(resources?.endpoints ?? []).filter(e => !e.attached).map(e => ({ kind: "endpoint" as const, id: e.id, name: e.name, text: `Endpoint · up to ${e.workersMax} workers` })),
  ]
  return <>
    {outside.length ? <section className="neo-panel" aria-label="Not in Yougori yet">
      <p className="neo-h3">In your account, not here yet</p>
      {outside.map(r => <div key={r.id} className="neo-item"><span>{r.name}<small>{r.text}</small></span>
        <Button size="xs" variant="outline" disabled={Boolean(working)} loading={working === r.id} onClick={() => void act(r.id, () => runpodApi.attach(r.kind, r.id), () => { void refreshPlatform(); onAttached() })}>Add here</Button></div>)}
    </section> : null}
    <section className="neo-panel" aria-label="Network volumes">
      <p className="neo-h3">Network volumes{catalog.volumes.length ? ` · ${money(catalog.volumes.reduce((sum, v) => sum + v.sizeGb, 0) * STORAGE_RATES.network)}/month` : ""}</p>
      {catalog.volumes.length ? catalog.volumes.map(v => <div key={v.id} className="neo-stack" style={{ gap: 6 }}>
        <div className="neo-item"><span>{v.name}<small>{v.sizeGb} GB · {v.location}</small></span>
          <span className="neo-row" style={{ gap: 4 }}>
            <Button size="xs" variant="ghost" disabled={Boolean(working)} onClick={() => setGrow(grow?.id === v.id ? null : { id: v.id, size: v.sizeGb * 2 })}>Grow</Button>
            {confirm === v.id
              ? <Button size="xs" variant="destructive" loading={working === v.id} disabled={Boolean(working)} onClick={() => void act(v.id, () => runpodApi.volume("delete", { id: v.id }))}>Delete files</Button>
              : <Button size="xs" variant="ghost" disabled={Boolean(working)} onClick={() => setConfirm(v.id)}>Delete</Button>}
          </span>
        </div>
        {grow?.id === v.id ? <div className="neo-row">
          <Input className="neo-input w-24" aria-label={`New size for ${v.name} in GB`} type="number" min={v.sizeGb + 1} max={4000} value={grow.size} onChange={e => setGrow({ id: v.id, size: Number(e.target.value) || 0 })} />
          <Button size="xs" variant="outline" disabled={Boolean(working) || grow.size <= v.sizeGb || grow.size > 4000} loading={working === v.id} onClick={() => void act(v.id, () => runpodApi.volume("resize", { id: v.id, sizeGb: grow.size }), () => { setGrow(null); onChanged() })}>Grow to {grow.size} GB</Button>
        </div> : null}
      </div>) : <p className="neo-note">None yet.</p>}
    </section>
    <section className="neo-panel" aria-label="Private image logins">
      <p className="neo-h3">Private image logins</p>
      {catalog.registries.map(r => <div key={r.id} className="neo-item"><span>{r.name}</span>
        <Button size="xs" variant="ghost" disabled={Boolean(working)} loading={working === r.id} onClick={() => void act(r.id, () => runpodApi.registry("delete", { id: r.id }))}>Remove</Button></div>)}
      {registry ? <form className="neo-stack" onSubmit={event => { event.preventDefault(); const r = registry; void act("registry", () => runpodApi.registry("create", r), () => { setRegistry(null); onChanged() }) }}>
        <Field label="Name"><Input className="neo-input" value={registry.name} onChange={e => setRegistry({ ...registry, name: e.target.value })} placeholder="ghcr" /></Field>
        <Field label="User name"><Input className="neo-input" value={registry.username} onChange={e => setRegistry({ ...registry, username: e.target.value })} autoComplete="off" /></Field>
        <Field label="Password or token"><Input className="neo-input" type="password" value={registry.password} onChange={e => setRegistry({ ...registry, password: e.target.value })} autoComplete="off" /></Field>
        <span className="neo-row"><Button size="xs" type="submit" disabled={!validName(registry.name) || !registry.username || !registry.password} loading={working === "registry"}>Save login</Button><Button size="xs" type="button" variant="ghost" onClick={() => setRegistry(null)}>Cancel</Button></span>
      </form> : <button type="button" className="neo-link neo-note" onClick={() => setRegistry({ name: "", username: "", password: "" })}>Add a login for private container images</button>}
    </section>
    {error ? <p role="alert" className="neo-error">{error}</p> : null}
  </>
}

function VolumeWorkbench({ catalog, account, busy, error, onBack, onCreate }: { catalog: RunpodCatalog; account: RunpodAccount | null | undefined; busy: boolean; error: string; onBack(): void; onCreate(name: string, location: string, sizeGb: number): void }) {
  const [step, setStep] = useState(0)
  const [name, setName] = useState("models")
  const [location, setLocation] = useState("")
  const [size, setSize] = useState(50)
  // Places with the most GPUs in stock first: pods using the volume must run there.
  const places = catalog.locations.map(l => ({ ...l, gpus: catalog.gpus.filter(g => g.locations.some(x => x.id === l.id && x.stock !== "none")).map(g => g.name) }))
    .sort((a, b) => b.gpus.length - a.gpus.length)
  const place = places.find(p => p.id === location)
  const sized = validName(name) && size >= 1 && size <= 4000
  const order: Order = {
    kicker: "Network volume", title: ["Where should it live?", "Name and size"][step] ?? "",
    lede: ["Pods and endpoints that use the volume must run in the same data centre, so pick one with the GPUs you want.", "Volumes can grow later but not shrink."][step] ?? "",
    steps: ["Location", "Size"], step, setStep, complete: [Boolean(location), sized],
    main: step === 0 ? <div className="neo-grid">
      {places.map(p => <Tile key={p.id} selected={location === p.id} onSelect={() => setLocation(p.id)} title={p.country} label={`Use ${p.id}`}
        badge={p.gpus.length ? <Badge tone="good">{p.gpus.length} GPU types</Badge> : <Badge>No GPUs now</Badge>} meta={p.id}>
        {p.gpus.length ? `${p.gpus.slice(0, 4).join(", ")}${p.gpus.length > 4 ? "…" : ""}` : "Nothing in stock at the moment."}
      </Tile>)}
    </div> : <div className="neo-fields">
      <Field label="Name" error={validName(name) ? undefined : "Use 2–40 letters, numbers, hyphens or underscores."}><Input className="neo-input" value={name} onChange={e => setName(e.target.value)} maxLength={40} /></Field>
      <Field label="Size" hint={`About ${money(size * STORAGE_RATES.network)} a month.`}>
        <span className="neo-size"><Input className="neo-input" type="number" min={1} max={4000} value={size} aria-label="Size in GB" onChange={e => setSize(Number(e.target.value) || 0)} /><span className="neo-note">GB</span>
          {[50, 100, 250, 500].map(p => <button key={p} type="button" className="neo-pill" aria-pressed={size === p} onClick={() => setSize(p)}>{p} GB</button>)}</span>
      </Field>
    </div>,
    summary: [["Location", place ? `${place.country} · ${place.id}` : null], ["Size", `${size} GB`], ["Name", name || null]],
    price: { amount: size * STORAGE_RATES.network, unit: "month", lines: ["Billed until you delete it"], empty: "" },
    consent: `I understand this bills my account about ${money(size * STORAGE_RATES.network)} a month until I delete it.`,
    submitLabel: "Create volume", ready: Boolean(location) && sized, signature: JSON.stringify([name, location, size]),
    submit: () => onCreate(name, location, size),
  }
  return <OrderFrame order={order} account={account} busy={busy} error={error} onBack={onBack} />
}
