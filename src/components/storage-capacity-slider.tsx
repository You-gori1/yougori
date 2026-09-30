import { useEffect, useId, useState } from "react"
import { HardDriveIcon } from "lucide-react"
import "./dialogs/creation-resource-sliders.css"

export function StorageCapacitySlider({ value, min, max, disabled = false, container = false, onChange }: {
  value: number; min: number; max: number; disabled?: boolean; container?: boolean; onChange(value: number): void
}) {
  const id = useId()
  const [draft, setDraft] = useState(String(value))
  useEffect(() => setDraft(String(value)), [value])
  const edit = (text: string) => {
    setDraft(text)
    if (!/^\d+$/.test(text)) return
    const next = Number(text)
    if (Number.isSafeInteger(next) && next >= min && next <= max) onChange(next)
  }
  const finish = () => {
    const next = Number(draft)
    if (draft.trim() && Number.isSafeInteger(next)) onChange(Math.min(Math.floor(max), Math.max(min, next)))
    else setDraft(String(value))
  }
  const fill = max > min ? Math.max(0, Math.min(100, (value - min) / (max - min) * 100)) : 0
  return <fieldset disabled={disabled} className="creation-resource" data-resource="storage">
    <legend className="creation-resource-title"><HardDriveIcon aria-hidden="true" />{container ? "Storage · this container" : "Storage"}</legend>
    <div className="creation-resource-start">
      <label htmlFor={id}>{container ? "Writable storage limit" : "Disk capacity"}</label>
      <input id={id} type="range" min={min} max={max} step={1} value={value} className="creation-resource-slider"
        aria-label={container ? "Storage limit" : "Storage capacity"} aria-valuetext={`${value} GB`} aria-describedby={`${id}-help`}
        style={{ backgroundImage: `linear-gradient(to right, var(--resource-color) ${fill}%, var(--resource-track) ${fill}%)` }}
        onChange={event => onChange(Number(event.target.value))} />
      <div className="creation-resource-number-wrap">
        <input type="number" inputMode="numeric" className="creation-resource-number" min={min} max={Math.floor(max)} step={1} value={draft}
          aria-label="Storage size in GB" aria-describedby={`${id}-help`} onChange={event => edit(event.target.value)} onBlur={finish}
          onKeyDown={event => { if (event.key === "Enter") { event.preventDefault(); event.currentTarget.blur() } }} />
        <span>GB</span>
      </div>
      <div className="creation-resource-scale" aria-hidden="true"><span>{min} GB</span><span>{max} GB</span></div>
    </div>
    <p id={`${id}-help`} className="text-xs text-muted-foreground">Minimum {min} GB. {container ? "An independent limit for this container. Uses host space as files are written; space is not reserved in advance." : "Uses host space as files are written, not all at once. Disk capacity never shrinks automatically."}</p>
  </fieldset>
}
