import { useEffect, useId, useState } from "react"
import { startupApi } from "@/api/startup-api"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"

export function PreferencesStartupHealth({ environmentId }: { environmentId: string }) {
  const id = useId()
  const [enabled, setEnabled] = useState(false)
  const [port, setPort] = useState("3000")
  const [path, setPath] = useState("/health")
  const [status, setStatus] = useState("200")
  const [method, setMethod] = useState<"GET" | "POST">("GET")
  const [body, setBody] = useState("")
  const [timeout, setTimeoutSeconds] = useState(10)
  const [secret, setSecret] = useState("")
  const [busy, setBusy] = useState(true)
  const [error, setError] = useState("")
  const [saved, setSaved] = useState(false)
  useEffect(() => {
    let disposed = false
    void startupApi.getHealthCheck(environmentId).then(check => {
      if (disposed) return
      setEnabled(Boolean(check))
      if (check) { setPort(String(check.port)); setPath(check.path); setStatus(String(check.expected_status)); setSecret(check.bearer_secret ?? ""); setMethod(check.method); setBody(check.body ? JSON.stringify(check.body, null, 2) : ""); setTimeoutSeconds(check.timeout_seconds) }
    }).catch(reason => { if (!disposed) setError(String(reason)) }).finally(() => { if (!disposed) setBusy(false) })
    return () => { disposed = true }
  }, [environmentId])
  const save = async () => {
    setSaved(false)
    setError("")
    const number = Number(port), expected = Number(status)
    if (enabled && (!Number.isInteger(number) || number < 1 || number > 65535 || !Number.isInteger(expected) || expected < 100 || expected > 599 || !path.startsWith("/") || path.startsWith("//") || /[\r\n]/.test(path))) {
      setError("Enter a valid application port, path beginning with /, and expected HTTP status.")
      return
    }
    setBusy(true)
    try {
      const parsedBody = method === "POST" && body.trim() ? JSON.parse(body) as Record<string, unknown> : null
      await startupApi.setHealthCheck(environmentId, enabled ? { port: number, path, method, expected_status: expected, timeout_seconds: timeout, bearer_secret: secret.trim() || null, body: parsedBody } : null)
      setSaved(true)
    } catch (reason) { setError(String(reason)) } finally { setBusy(false) }
  }
  return <details className="preferences-note">
    <summary>Verify this application at startup</summary>
    <div className="preferences-row">
      <div><label htmlFor={`${id}-enabled`}>Check application health</label><p>Verify the app and its public HTTPS address after startup. Allow up to two minutes for the application to become ready.</p></div>
      <Switch id={`${id}-enabled`} checked={enabled} disabled={busy} onCheckedChange={value => { setEnabled(value); setSaved(false) }} />
    </div>
    {enabled && <fieldset disabled={busy} className="preferences-health-fields">
      <label htmlFor={`${id}-port`}>Application port</label><input id={`${id}-port`} type="number" min="1" max="65535" value={port} onChange={event => { setPort(event.target.value); setSaved(false) }} />
      <label htmlFor={`${id}-path`}>Health check path</label><input id={`${id}-path`} value={path} onChange={event => { setPath(event.target.value); setSaved(false) }} />
      <label htmlFor={`${id}-method`}>Request method</label><select id={`${id}-method`} value={method} onChange={event => { setMethod(event.target.value as "GET" | "POST"); setSaved(false) }}><option>GET</option><option>POST</option></select>
      {method === "POST" && <><label htmlFor={`${id}-body`}>JSON request body (optional)</label><textarea id={`${id}-body`} value={body} onChange={event => { setBody(event.target.value); setSaved(false) }} /><p>Use a harmless test request. Startup may repeat this request while the app becomes ready.</p></>}
      <label htmlFor={`${id}-status`}>Expected HTTP status</label><input id={`${id}-status`} type="number" min="100" max="599" value={status} onChange={event => { setStatus(event.target.value); setSaved(false) }} />
      <label htmlFor={`${id}-secret`}>Saved secret name (optional)</label><input id={`${id}-secret`} value={secret} placeholder="api-health-token" onChange={event => { setSecret(event.target.value); setSaved(false) }} />
      <p>Use a deployment secret already stored in the vault. Enter its name here.</p>
    </fieldset>}
    <Button size="sm" disabled={busy} loading={busy} onClick={() => void save()}>Save health check</Button>
    {saved && <p role="status">Health check saved.</p>}
    {error && <p role="alert" className="preferences-error">{error}</p>}
  </details>
}
