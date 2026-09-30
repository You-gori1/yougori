import { useImperativeHandle, useState, type Ref } from "react"
import { Input } from "@/components/ui/input"
import { Field, FieldLabel, FieldDescription } from "@/components/ui/field"
import { usePlatform } from "@/context/platform-context"
import type { SectionSave } from "@/lib/resource-controls"
import type { Environment } from "@/types/platform"

/** Edits a draft; the configuration's Save changes button stores it through `saveRef`. */
export function EnvironmentNameEditor({ environment, saveRef, disabled = false, onSubmit }: {
  environment: Environment; saveRef?: Ref<SectionSave>; disabled?: boolean; onSubmit?(): void
}) {
  const { renameEnvironment } = usePlatform()
  // Keep the draft separate from polling updates; null follows the saved name.
  const [draft, setDraft] = useState<string | null>(null)
  const name = draft ?? environment.name
  const trimmed = name.trim()
  const valid = [...trimmed].length >= 2 && [...trimmed].length <= 80 && !/\p{Cc}/u.test(trimmed)
  useImperativeHandle(saveRef, () => ({
    changed: () => trimmed !== environment.name,
    problem: () => valid ? null : "Use 2–80 characters for the VM name, without control characters.",
    save: async () => { await renameEnvironment(environment.id, trimmed); setDraft(null) },
  }), [environment.id, environment.name, renameEnvironment, trimmed, valid])
  return <Field>
    <FieldLabel htmlFor="environment-display-name">VM name</FieldLabel>
    <Input id="environment-display-name" value={name} disabled={disabled} aria-invalid={!valid} onChange={event => setDraft(event.target.value)} onKeyDown={event => { if (event.key === "Enter") { event.preventDefault(); onSubmit?.() } }} />
    <FieldDescription>Changes the display name only. No restart needed.</FieldDescription>
    {!valid ? <p role="alert" className="text-xs text-destructive-foreground">Use 2–80 characters, without control characters.</p> : null}
  </Field>
}
