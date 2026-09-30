import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import type { VaultItemInput } from "@/api/vault-api"

type Field = { name: string; value: string }
const kinds = [
  ["password", "Password"], ["api_key", "API key"], ["token", "Token"],
  ["private_key", "Private key"], ["credential", "Other credential"],
  ["personal", "Personal information"], ["cloudflare", "Cloudflare DNS"],
] as const

export function VaultItemEditor({ onSave, onCancel }: { onSave: (items: VaultItemInput[]) => Promise<void>; onCancel: () => void }) {
  const [label, setLabel] = useState("")
  const [kind, setKind] = useState("password")
  const [resource, setResource] = useState("")
  const [value, setValue] = useState("")
  const [fields, setFields] = useState<Field[]>([{ name: "", value: "" }])
  const [queued, setQueued] = useState<VaultItemInput[]>([])
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState("")
  const collect = (): VaultItemInput => {
    const name = label.trim()
    if (!name) throw new Error("Enter an item name")
    if (kind === "personal") {
      const entries = fields.map(field => [field.name.trim(), field.value] as const)
      if (entries.some(([key, text]) => !key || !text) || new Set(entries.map(([key]) => key)).size !== entries.length) {
        throw new Error("Give each personal field a unique name and value")
      }
      return { label: name, kind, resource: "", value: JSON.stringify(Object.fromEntries(entries)) }
    }
    if (!value) throw new Error("Enter a value")
    if (kind === "cloudflare" && !/^[a-fA-F0-9]{32}$/.test(resource.trim())) throw new Error("Enter the 32-character Cloudflare zone ID")
    return { label: name, kind, resource: kind === "cloudflare" ? resource.trim() : "", value }
  }
  const clear = () => { setLabel(""); setKind("password"); setResource(""); setValue(""); setFields([{ name: "", value: "" }]); setError("") }
  const addAnother = () => { try { if (queued.length >= 127) throw new Error("The vault supports up to 128 items at once"); setQueued([...queued, collect()]); clear() } catch (reason) { setError(String(reason)) } }
  const save = async () => {
    try {
      setError("")
      const hasCurrent = Boolean(label || value || (kind === "personal" && fields.some(field => field.name || field.value)))
      const current = hasCurrent ? [collect()] : []
      const items = [...queued, ...current]
      if (!items.length) throw new Error("Add at least one item")
      setSaving(true)
      await onSave(items)
      clear(); setQueued([])
    } catch (reason) { setError(String(reason)) }
    finally { setSaving(false) }
  }
  return <section aria-label="Add vault items" className="mx-auto max-w-2xl space-y-5 p-6 sm:p-8">
    <div><h2 className="text-xl font-semibold">Add private items</h2><p className="mt-1 text-xs leading-5 text-muted-foreground">Enter items here in Yougori. They are sent to the local vault broker for encrypted storage. Values are cleared from this form after saving or closing it.</p></div>
    {queued.length > 0 && <div className="rounded-lg border p-3 text-xs"><p className="font-medium">Ready to save · {queued.length}</p><div className="mt-2 space-y-1 text-muted-foreground">{queued.map((item, index) => <div key={index} className="flex items-center justify-between gap-3"><span>{item.label} · {item.kind.replaceAll("_", " ")}</span><button type="button" className="text-destructive" onClick={() => setQueued(list => list.filter((_, i) => i !== index))}>Remove</button></div>)}</div></div>}
    <div className="grid gap-4 sm:grid-cols-2">
      <label className="space-y-1.5 text-xs font-medium">Name<Input aria-label="Item name" value={label} maxLength={100} autoComplete="off" onChange={event => setLabel(event.target.value)} disabled={saving} /></label>
      <label className="space-y-1.5 text-xs font-medium">Type<select aria-label="Item type" className="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm" value={kind} onChange={event => setKind(event.target.value)} disabled={saving}>{kinds.map(([id, text]) => <option key={id} value={id}>{text}</option>)}</select></label>
    </div>
    {kind === "cloudflare" && <label className="block space-y-1.5 text-xs font-medium">Cloudflare zone ID<Input aria-label="Cloudflare zone ID" value={resource} maxLength={32} autoComplete="off" onChange={event => setResource(event.target.value)} disabled={saving} /></label>}
    {kind === "personal" ? <div className="space-y-3"><p className="text-xs font-medium">Fields</p>{fields.map((field, index) => <div key={index} className="flex gap-2"><Input aria-label={`Field ${index + 1} name`} placeholder="Field name" value={field.name} maxLength={100} autoComplete="off" disabled={saving} onChange={event => setFields(list => list.map((entry, i) => i === index ? { ...entry, name: event.target.value } : entry))} /><Input aria-label={`Field ${index + 1} value`} placeholder="Value" value={field.value} maxLength={4096} autoComplete="off" disabled={saving} onChange={event => setFields(list => list.map((entry, i) => i === index ? { ...entry, value: event.target.value } : entry))} />{fields.length > 1 && <Button type="button" size="xs" variant="ghost" disabled={saving} onClick={() => setFields(list => list.filter((_, i) => i !== index))}>Remove</Button>}</div>)}<Button type="button" size="xs" variant="outline" disabled={saving || fields.length >= 64} onClick={() => setFields(list => [...list, { name: "", value: "" }])}>Add field</Button></div> : <label className="block space-y-1.5 text-xs font-medium">{kind === "cloudflare" ? "API token" : "Value"}{kind === "private_key" ? <Textarea aria-label="Item value" value={value} maxLength={32768} autoComplete="off" spellCheck={false} disabled={saving} rows={6} className="font-mono text-xs" onChange={event => setValue(event.target.value)} /> : <Input aria-label="Item value" type="password" value={value} maxLength={32768} autoComplete="off" spellCheck={false} disabled={saving} onChange={event => setValue(event.target.value)} />}</label>}
    {error && <p role="alert" className="text-xs text-destructive">{error}</p>}
    <div className="flex flex-wrap justify-end gap-2"><Button type="button" variant="ghost" disabled={saving} onClick={onCancel}>Cancel</Button><Button type="button" variant="outline" disabled={saving} onClick={addAnother}>Add another</Button><Button type="button" disabled={saving} onClick={() => void save()}>{saving ? "Saving..." : queued.length ? "Save items" : "Save item"}</Button></div>
  </section>
}
