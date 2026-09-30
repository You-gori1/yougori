import { useId, type CSSProperties, type ReactNode } from "react"
import { ConfigurationHelp } from "@/components/configuration-help"
import "./resource-value-control.css"

export function ResourceValueControl({ label, value, min, max, step = 1, unit, disabled = false, help, onChange }: {
  label: string; value: number; min: number; max: number; step?: number; unit: string; disabled?: boolean; help?: ReactNode; onChange(value: number): void
}) {
  const id = useId()
  const valid = Number.isFinite(value) && value >= min && value <= max && Math.abs((value - min) / step - Math.round((value - min) / step)) < 1e-6
  const position = Number.isFinite(value) ? Math.max(min, Math.min(max, value)) : min
  const fill = max > min ? (position - min) / (max - min) * 100 : 0
  return <div className="resource-value-control">
    <div className="resource-value-heading">
      <label htmlFor={`${id}-value`}>{label}</label>
      {help ? <ConfigurationHelp label={`${label} help`}>{help}</ConfigurationHelp> : null}
      <div className="resource-value-input">
        <input id={`${id}-value`} aria-label={`${label} value`} aria-invalid={!valid || undefined} type="number" inputMode="decimal" min={min} max={max} step={step} value={Number.isFinite(value) ? value : ""} disabled={disabled} onChange={event => onChange(event.target.value === "" ? Number.NaN : Number(event.target.value))} />
        <span>{unit}</span>
      </div>
    </div>
    <input className="resource-value-track" aria-label={`${label} allocation`} aria-valuetext={`${position} ${unit}`} type="range" min={min} max={max} step={step} value={position} disabled={disabled || max <= min} style={{ "--resource-fill": `${fill}%` } as CSSProperties} onChange={event => onChange(Number(Number(event.target.value).toFixed(6)))} />
  </div>
}
