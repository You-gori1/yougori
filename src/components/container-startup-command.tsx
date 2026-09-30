import { TerminalIcon } from "lucide-react"
import { Field, FieldLabel } from "@/components/ui/field"
import { ConfigurationHelp } from "@/components/configuration-help"
import { Textarea } from "@/components/ui/textarea"
import type { Environment } from "@/types/platform"

/** Edits a draft only; the configuration's Save changes button stores it. */
export function ContainerStartupCommand({ environment, draft, disabled, onDraftChange }: {
  environment: Environment; draft: string | null; disabled: boolean; onDraftChange(command: string): void
}) {
  const savedCommand = environment.containerCommand ?? ""
  const command = draft ?? savedCommand
  const changed = command.trim() !== savedCommand
  return <section className="inspector-section" aria-label="Container startup">
    <div className="inspector-section-heading"><h3 className="inspector-section-title"><TerminalIcon aria-hidden="true" />Startup</h3><ConfigurationHelp label="Startup command help">Runs inside this container each time it starts. Leave empty to use the image’s default startup. Keep your main process in the foreground; the container stops when it exits.</ConfigurationHelp></div>
    <Field>
      <FieldLabel>Startup command</FieldLabel>
      <Textarea className="font-mono text-xs [&_textarea]:field-sizing-fixed [&_textarea]:h-28 [&_textarea]:min-h-0 [&_textarea]:resize-none [&_textarea]:overflow-y-auto [&_textarea]:overscroll-contain" rows={3} maxLength={32768} disabled={disabled} value={command} placeholder="Use the image’s default startup" onChange={event => onDraftChange(event.target.value)} />
    </Field>
    {changed && environment.status !== "stopped" ? <p className="inspector-description">Stop the container, then Save changes to use this startup command.</p> : null}
  </section>
}
