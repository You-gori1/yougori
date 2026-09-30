import { useEffect, useMemo, useState, type FormEvent } from "react"
import { Button } from "@/components/ui/button"
import { Dialog, DialogClose, DialogDescription, DialogFooter, DialogHeader, DialogPanel, DialogPopup, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Form } from "@/components/ui/form"
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from "@/components/ui/select"
import { usePlatform } from "@/context/platform-context"

export function BackupDialog({ open, onOpenChange, initialEnvironmentId }: { open: boolean; onOpenChange(open: boolean): void; initialEnvironmentId?: string }) {
  const { state, runBackup } = usePlatform()
  const environmentOptions = useMemo(() => state?.environments.map((item) => ({ label: item.name, value: item.id })) ?? [], [state?.environments])
  const destinationOptions = useMemo(() => state?.destinations.filter((item) => item.connected).map((item) => ({ label: item.name, value: item.id })) ?? [], [state?.destinations])
  const [environmentId, setEnvironmentId] = useState("")
  const [destinationId, setDestinationId] = useState("")
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (!open) return
    setEnvironmentId(initialEnvironmentId ?? environmentOptions[0]?.value ?? "")
    setDestinationId(destinationOptions[0]?.value ?? "")
  }, [destinationOptions, environmentOptions, initialEnvironmentId, open])

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); setSaving(true)
    try { await runBackup(environmentId, destinationId); onOpenChange(false) } finally { setSaving(false) }
  }

  const selectedEnvironment = environmentOptions.find((item) => item.value === environmentId)
  const selectedDestination = destinationOptions.find((item) => item.value === destinationId)

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogPopup className="sm:max-w-md">
        <DialogHeader><DialogTitle>Back up environment</DialogTitle><DialogDescription>Create an immutable snapshot, deduplicate it, then encrypt and upload only changed blocks.</DialogDescription></DialogHeader>
        <Form className="contents" onSubmit={submit}>
          <DialogPanel className="flex flex-col gap-5">
            <Field><FieldLabel>Environment</FieldLabel>
              <Select itemToStringValue={(item) => item.value} items={environmentOptions} onValueChange={(item) => setEnvironmentId(item?.value ?? "")} value={selectedEnvironment}>
                <SelectTrigger><SelectValue placeholder="Choose environment" /></SelectTrigger><SelectPopup>{environmentOptions.map((item) => <SelectItem key={item.value} value={item}>{item.label}</SelectItem>)}</SelectPopup>
              </Select>
            </Field>
            <Field><FieldLabel>Destination</FieldLabel>
              <Select itemToStringValue={(item) => item.value} items={destinationOptions} onValueChange={(item) => setDestinationId(item?.value ?? "")} value={selectedDestination}>
                <SelectTrigger><SelectValue placeholder="Choose destination" /></SelectTrigger><SelectPopup>{destinationOptions.map((item) => <SelectItem key={item.value} value={item}>{item.label}</SelectItem>)}</SelectPopup>
              </Select>
              <FieldDescription>The restore point includes OS/data, connections, networks, resource policies, and configuration.</FieldDescription>
            </Field>
          </DialogPanel>
          <DialogFooter><DialogClose render={<Button type="button" variant="ghost" />}>Cancel</DialogClose><Button disabled={!environmentId || !destinationId} loading={saving} type="submit">Back up now</Button></DialogFooter>
        </Form>
      </DialogPopup>
    </Dialog>
  )
}
