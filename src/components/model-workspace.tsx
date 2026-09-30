import { useEffect, useRef, useState } from "react"
import { ModelChat } from "@/components/model-chat"
import { ModelApiPanel } from "@/components/model-api-panel"
import { ModelUsagePanel } from "@/components/model-usage-panel"
import { modelsApi } from "@/api/projects-api"
import { usePlatform } from "@/context/platform-context"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { Dialog, DialogClose, DialogTrigger, DialogPopup, DialogTitle, DialogDescription, DialogPanel } from "@/components/ui/dialog"
import { useTopicWalkthroughModal } from "@/lib/topic-walkthrough"
import { toastManager } from "@/components/ui/toast"
import "@/components/model-workspace.css"

export { ModelChat }

/** `compact` renders a small "Chat" trigger for use inside a graph node. */
export function ModelWorkspace({ environmentId, compact = false }: { environmentId?: string; compact?: boolean }) {
  const { state, refreshPlatform } = usePlatform()
  const [open, setOpen] = useState(false)
  const topic = useTopicWalkthroughModal(open)
  const [model, setModel] = useState("hf.co/TinyLlama/TinyLlama-1.1B-Chat-v1.0")
  const [api, setApi] = useState(false)
  const [port, setPort] = useState("8000")
  const [selected, setSelected] = useState(environmentId ?? "")
  const [view, setView] = useState<"chat" | "api" | "usage">("chat")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const lock = useRef(false)
  const [launching, setLaunching] = useState(false)
  const [elapsed, setElapsed] = useState(0)
  const previousIds = useRef(new Set<string>())
  useEffect(() => {
    if (!launching) return
    let active = true, timer = 0
    const started = Date.now()
    const clock = window.setInterval(() => setElapsed(Math.floor((Date.now() - started) / 1000)), 1000)
    const poll = async () => {
      try { await refreshPlatform() } catch { /* The launch reports its own errors. */ }
      finally { if (active) timer = window.setTimeout(poll, 3000) }
    }
    void poll()
    return () => { active = false; window.clearTimeout(timer); window.clearInterval(clock) }
  }, [launching, refreshPlatform])
  const normalizedModel = model.trim().replace(/^(https:\/\/huggingface.co\/|hf.co\/)/, "")
  const pendingModel = launching ? state?.environments.find(e => !previousIds.current.has(e.id) && e.description === `Hugging Face · ${normalizedModel}`) : undefined
  const visibleId = selected || pendingModel?.id
  const models = state?.environments.filter(e => e.description.startsWith("Hugging Face · ")) ?? []
  const launch = async () => {
    if (lock.current) return
    previousIds.current = new Set(state?.environments.map(e => e.id))
    lock.current = true; setBusy(true); setLaunching(true); setElapsed(0); setError(""); setOpen(false)
    try { const result = await modelsApi.run(model, api ? Number(port) : null); setSelected(result.id); setView("chat"); await refreshPlatform() }
    catch (e) { setError(String(e)); toastManager.add({ title: "Model setup needs attention", description: String(e), type: "error" }); await refreshPlatform().catch(() => undefined) }
    finally { setBusy(false); setLaunching(false); lock.current = false }
  }
  const showTabs = !environmentId && models.length > 0
  const close = <DialogClose render={<Button variant="outline" size="sm" className="model-close" />}>Close</DialogClose>
  const validPort = Number.isInteger(Number(port)) && Number(port) >= 1 && Number(port) <= 65535
  return <Dialog modal={!topic} open={open} onOpenChange={(value, details) => { if (!(!value && topic && details.reason === "focus-out") && !(details.event.target instanceof Element && details.event.target.closest('[data-topic-ui]'))) setOpen(value) }}>
    <DialogTrigger render={compact ? <Button size="xs" variant="ghost" className="node-chat nodrag" /> : <Button data-instruction="model-trigger" size="sm" variant="outline" className={environmentId ? undefined : "dashboard-action"} />}>{compact ? "Chat" : environmentId ? "Chat with model" : "Huggingface"}</DialogTrigger>
    <DialogPopup data-instruction="model-dialog" bottomStickOnMobile={false} showCloseButton={false} className={`model-popup max-w-none${visibleId ? " model-popup-chat" : ""}`}>
      <DialogTitle className="sr-only">Hugging Face</DialogTitle>
      <DialogDescription className="sr-only">Run a language model on your NVIDIA GPU.</DialogDescription>
      {showTabs || !selected ? <header className="model-header">
        {showTabs ? <div className="model-tabs" role="group" aria-label="Your models">
          {models.map(e => <button key={e.id} type="button" aria-pressed={selected === e.id} disabled={busy} onClick={() => setSelected(e.id)}>{e.description.replace("Hugging Face · ", "")}</button>)}
          <button type="button" aria-pressed={!selected} disabled={busy} onClick={() => setSelected("")}>{selected ? "Run another model" : "New model"}</button>
        </div> : null}
        {selected ? null : close}
      </header> : null}
      <DialogPanel className="model-panel" scrollFade={false}>
        {!environmentId && !selected ? <div className="model-form">
          <label className="model-label" htmlFor="hf-model">Model</label>
          <Input id="hf-model" value={model} disabled={busy} onChange={e => setModel(e.target.value)} placeholder="hf.co/owner/model" />
          <p className="model-hint">Public text-generation models with safetensors. Needs enough VRAM.</p>
          <div className="model-api-row">
            <div><label htmlFor="hf-api">Local API</label><p>OpenAI-compatible, this PC only.</p></div>
            <div className="model-api-controls">
              {api ? <Input className="w-24" aria-label="Model API port" type="number" min={1} max={65535} value={port} disabled={busy} onChange={e => setPort(e.target.value)} /> : null}
              <Switch id="hf-api" checked={api} disabled={busy} onCheckedChange={setApi} />
            </div>
          </div>
          <div className="model-form-actions"><Button disabled={busy || !model.trim() || (api && !validPort)} loading={busy} onClick={() => void launch()}>Run model</Button></div>
        </div> : null}

        {selected ? <div className="model-view-row">
          <div className="model-tabs" role="group" aria-label="View">
            <button type="button" aria-pressed={view === "chat"} onClick={() => setView("chat")}>Chat</button>
            <button type="button" aria-pressed={view === "api"} onClick={() => setView("api")}>API access</button>
            <button type="button" aria-pressed={view === "usage"} onClick={() => setView("usage")}>Usage</button>
          </div>
          {close}
        </div> : null}

        {visibleId && open ? view === "api" && selected ? <ModelApiPanel key={visibleId} environmentId={visibleId} /> : view === "usage" && selected ? <ModelUsagePanel key={visibleId} environmentId={visibleId} /> : <ModelChat key={visibleId} environmentId={visibleId} /> : null}
        {error ? <p role="alert" className="model-error">{error}</p> : null}
        {busy ? <p role="status" className="model-hint">{launching ? `${pendingModel ? "Starting the model" : "Preparing GPU runtime"}… ${Math.floor(elapsed / 60)}m ${elapsed % 60}s` : null}</p> : null}
      </DialogPanel>
    </DialogPopup>
  </Dialog>
}
