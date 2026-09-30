import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"

export function ResourceRangeFields({ label, unit, value, min, max, step, scale = 1, disabled = false, onChange }: {
  label: string; unit: string; value: number[]; min: number; max: number; step: number; scale?: number; disabled?: boolean; onChange(value: number[]): void
}) {
  return <div role="group" aria-label={`${label} allocation`} className="flex flex-col gap-2">
    <div className="flex items-center justify-between gap-2"><p className="text-sm font-medium">{label}</p><span className="text-[11px] text-muted-foreground">{min * scale}–{Number((max * scale).toFixed(6))} {unit}</span></div>
    <div className="grid grid-cols-3 gap-2">
      {["Minimum", "Preferred", "Maximum"].map((name, index) => <Field key={name}>
        <FieldLabel className="text-[11px] font-normal text-muted-foreground">{name} ({unit})</FieldLabel>
        <Input type="number" required disabled={disabled} min={min * scale} max={max * scale} step={step * scale}
          className="min-w-0 tabular-nums" value={Number.isFinite(value[index]) ? Number((value[index]! * scale).toFixed(6)) : ""}
          onChange={event => onChange(value.map((current, position) => position === index ? (event.target.value === "" ? NaN : Number(event.target.value) / scale) : current))} />
      </Field>)}
    </div>
  </div>
}
