import { useEffect, useRef, useState } from "react"
import { gpuApi, type CudaRuntimeStatus } from "@/api/gpu-api"
import { Button } from "@/components/ui/button"

export function CudaRuntimePanel({ onStatus, storageDrive, onBusyChange }: { storageDrive?: string; onBusyChange?(busy: boolean): void; onStatus?(status: CudaRuntimeStatus): void }) {
  const mounted = useRef(true)
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  const [status, setStatus] = useState<CudaRuntimeStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [revision, setRevision] = useState(0)
  const [checking, setChecking] = useState(true)
  useEffect(() => {
    let active = true
    setChecking(true); setError("")
    onStatus?.({ supported: false, installed: false, running: false, detail: "Checking this computer before creating a GPU environment…" })
    void gpuApi.cudaStatus(storageDrive).then(next => { if (active) { setStatus(next); onStatus?.(next) } }).catch(reason => { if (active) {
      setError(String(reason)); setStatus(null)
      onStatus?.({ supported: false, installed: false, running: false, detail: "Compatibility could not be checked. Recheck before creating a GPU environment." })
    } }).finally(() => { if (active) setChecking(false) })
    return () => { active = false }
  }, [onStatus, revision, storageDrive])
  const install = async () => {
    if (busy) return
    setBusy(true); onBusyChange?.(true); setError("")
    try { const next = await gpuApi.installCuda(storageDrive); if (mounted.current) { setStatus(next); onStatus?.(next) } }
    catch (reason) { setError(String(reason)) }
    finally { setBusy(false); onBusyChange?.(false) }
  }
  return <section className="space-y-3 rounded-lg border px-4 py-3" aria-label="NVIDIA CUDA runtime" aria-busy={busy || checking}>
    <div className="flex items-center justify-between gap-3"><h2 className="text-sm font-semibold">NVIDIA CUDA · containers</h2>
      {status?.supported && status.installed && !status.updateAvailable ? <span className="text-xs text-muted-foreground">{status.running ? "Runtime running" : "Installed"}</span> : <Button disabled={checking || busy || !status?.supported || status.running} loading={busy} onClick={() => void install()} size="sm" type="button">{status?.updateAvailable ? "Update CUDA" : "Set up CUDA"}</Button>}
    </div>
    <p className="text-xs text-muted-foreground">{busy ? "Installing the isolated CUDA runtime. Downloads may take several minutes. Existing environments are not moved." : status?.detail || "Checking CUDA runtime…"}</p>
    {status?.checks?.length ? <ul aria-label="Computer compatibility" className="space-y-2 text-xs">{status.checks.map(check => <li key={check.name}><span className={check.passed ? "text-emerald-500" : "text-destructive"}>{check.passed ? "✓" : "!"} {check.name}</span><p className="mt-0.5 text-muted-foreground">{check.detail}</p></li>)}</ul> : null}
    <Button type="button" size="xs" variant="ghost" disabled={busy || checking} onClick={() => setRevision(value => value + 1)}>{checking ? "Checking computer…" : "Recheck computer"}</Button>
    <p className="text-xs text-muted-foreground">GPU access is included in new GPU environments. Internet and My PC folders stay disconnected. Uses Ubuntu, Debian or compatible glibc images; stock Alpine is not CUDA-ready.</p>
    <p className="text-xs text-muted-foreground">Windows x64 + WSL 2 + compatible NVIDIA GPU required. Intel or AMD CPUs are fine; AMD/Intel-only GPUs and Apple GPUs cannot run CUDA. Native Linux/macOS GPU backends are not implemented.</p>
    <p className="text-xs text-muted-foreground">This is a container with separate storage and a shared WSL kernel, not a dedicated VM. All NVIDIA devices exposed by WSL are available; per-GPU selection and reserved GPU memory are not implemented. Applications may require newer drivers than these prerequisite checks.</p>
    {error ? <p role="alert" className="text-xs text-destructive">{error}</p> : null}
  </section>
}
