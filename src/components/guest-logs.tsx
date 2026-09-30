import { useCallback, useEffect, useRef, useState } from "react"
import { workspaceApi } from "@/api/workspace-api"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"

// Recent workload or VM runtime logs for the selected environment.
export function GuestLogs({ environmentId, active }: { environmentId: string; active: boolean }) {
  return <EnvironmentLogs key={environmentId} environmentId={environmentId} active={active} />
}

function EnvironmentLogs({ environmentId, active }: { environmentId: string; active: boolean }) {
  const [text, setText] = useState("")
  const [error, setError] = useState("")
  const [follow, setFollow] = useState(true)
  const [loading, setLoading] = useState(false)
  const output = useRef<HTMLPreElement>(null)
  const requestScope = useRef<{ active: boolean; pending: Promise<void> | null }>({ active: false, pending: null })
  const load = useCallback(() => {
    const scope = requestScope.current
    if (!scope.active) return Promise.resolve()
    if (scope.pending) return scope.pending
    setLoading(true)
    scope.pending = workspaceApi.logs(environmentId)
      .then(text => { if (scope.active) { setText(text); setError("") } })
      .catch(reason => { if (scope.active) setError(reason instanceof Error ? reason.message : String(reason)) })
      .finally(() => { scope.pending = null; if (scope.active) setLoading(false) })
    return scope.pending
  }, [environmentId])
  useEffect(() => {
    setLoading(false)
    if (!active) return
    const scope = { active: true, pending: null }; requestScope.current = scope
    let disposed = false, timer = 0
    const tick = () => void load().finally(() => { if (!disposed && follow) timer = window.setTimeout(tick, 4000) })
    tick()
    return () => { disposed = true; scope.active = false; window.clearTimeout(timer) }
  }, [active, follow, load])
  useEffect(() => { if (follow && output.current) output.current.scrollTop = output.current.scrollHeight }, [text, follow])

  return <div data-guest-logs className="flex h-full min-h-0 flex-col gap-2 p-2">
    <div className="flex flex-wrap items-center gap-3 text-xs">
      <Button size="xs" variant="outline" disabled={!active || loading} onClick={() => void load()}>Refresh</Button>
      <label className="flex items-center gap-2"><Checkbox checked={follow} onCheckedChange={value => setFollow(value === true)} />Keep refreshing</label>
      <span className="text-muted-foreground">Recent guest workload and runtime output.</span>
    </div>
    {error ? <p role="alert" className="break-words text-xs text-destructive-foreground">{error}</p> : null}
    <pre ref={output} aria-label="Environment logs" className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-words rounded border bg-[#0c0c0c] p-3 font-mono text-xs text-[#cccccc]">{text || (error ? "Logs are unavailable." : loading ? "Loading logs…" : "No logs yet.")}</pre>
  </div>
}
