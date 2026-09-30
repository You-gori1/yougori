import { useState } from "react"
import { gpuApi } from "@/api/gpu-api"
import type { Environment } from "@/types/platform"
import { Button } from "@/components/ui/button"

export function CudaVerification({ environment, disabled = false, onEnableGpu }: { environment: Environment; disabled?: boolean; onEnableGpu?(): Promise<unknown> }) {
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<{ passed: boolean; text: string } | null>(null)
  const cuda = environment.provider === "yougoriCuda"
  const test = async () => {
    if (busy) return
    setBusy(true); setResult(null)
    try {
      const reply = await gpuApi.verifyCuda(environment.id)
      const passed = reply.exitCode === 0 && reply.stdout.includes("CUDA KERNEL PASS:")
      setResult({ passed, text: passed ? reply.stdout.trim() : `${reply.stderr || reply.stdout || "The GPU calculation did not pass."}\nThe built-in check needs a compatible glibc image (Ubuntu 22.04+ or Debian 12+). Stock Alpine cannot run it. Check the Windows NVIDIA driver and GPU setup in New environment → Container → GPU access.` })
    } catch (reason) { setResult({ passed: false, text: String(reason) }) }
    finally { setBusy(false) }
  }
  return <section aria-label="GPU and CUDA" className="inspector-section">
    <div className="flex items-center justify-between gap-3"><h3 className="text-sm font-semibold">GPU / CUDA</h3>
      {cuda ? <Button disabled={disabled || busy || environment.status !== "running" || !environment.gpuAccess} loading={busy} onClick={() => void test()} size="sm" type="button" variant="outline">Test CUDA</Button> : null}
    </div>
    <p className="inspector-description">{cuda
      ? "NVIDIA CUDA engine. The check runs a small calculation inside this container as a non-root user and verifies all 256 results. No Python or CUDA toolkit installation is needed for the check."
      : environment.kind === "container" ? "Standard container: no CUDA. Create a GPU environment for NVIDIA CUDA, or restore a local container backup into the GPU category. Existing files are never moved automatically."
      : environment.kind === "microVm" ? "This lightweight MicroVM engine does not expose a GPU or CUDA device."
      : "This QEMU engine can provide compatible Linux graphics, but not CUDA or Windows GPU acceleration. Use a GPU environment for NVIDIA CUDA computing."}</p>
    {cuda && !environment.gpuAccess ? <><p className="inspector-description">This older or restored GPU environment has access disabled. Stop it, then enable GPU access here. Its files and other permissions are kept.</p><Button size="sm" variant="outline" disabled={disabled || busy || environment.status !== "stopped" || !onEnableGpu} loading={busy} onClick={() => {
      if (!onEnableGpu || busy) return
      setBusy(true); setResult(null)
      void onEnableGpu().catch(reason => setResult({ passed: false, text: String(reason) })).finally(() => setBusy(false))
    }}>Enable GPU access</Button></> : cuda && environment.status !== "running" ? <p className="inspector-description">GPU access is included. Start this environment to test CUDA.</p> : null}
    {result ? <p role={result.passed ? "status" : "alert"} className={`mt-2 whitespace-pre-wrap break-words text-xs ${result.passed ? "text-emerald-500" : "text-destructive"}`}>{result.text}</p> : null}
  </section>
}
