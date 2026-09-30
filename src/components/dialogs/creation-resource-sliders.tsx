import { useId } from "react"
import { CpuIcon, MemoryStickIcon } from "lucide-react"
import "./creation-resource-sliders.css"

function updateResourceRange(value: number[], index: number, next: number): number[] {
  return value.map((current, position) => position < index ? Math.min(current, next) : position > index ? Math.max(current, next) : next)
}

export function CreationResourceSliders({ label, unit, value, min, max, step, disabled, onChange }: {
  label: string; unit: string; value: number[]; min: number; max: number; step: number; disabled: boolean; onChange(value: number[]): void
}) {
  const id = useId()
  const Icon = label === "CPU" ? CpuIcon : MemoryStickIcon
  return <fieldset disabled={disabled} className="creation-resource" data-resource={label.toLowerCase()}>
    <legend className="creation-resource-title"><Icon aria-hidden="true" />{label}</legend>
    {[1, 0, 2].map(index => {
      const name = ["Minimum", "Preferred", "Maximum"][index]!
      const current = value[index]!
      const fill = max > min ? ((current - min) / (max - min)) * 100 : 0
      return <div key={name} className={index === 1 ? "creation-resource-start" : "creation-resource-bound"}>
        <label htmlFor={`${id}-${index}`}>{index === 1 ? "Preferred · at startup" : name}</label>
        <input id={`${id}-${index}`} className="creation-resource-slider" type="range" min={min} max={max} step={step} value={current}
          aria-label={`${label} ${name.toLowerCase()}`} aria-valuetext={`${current} ${unit}`}
          style={{ backgroundImage: `linear-gradient(to right, var(--resource-color) ${fill}%, var(--resource-track) ${fill}%)` }}
          onChange={event => onChange(updateResourceRange(value, index, Number(Number(event.target.value).toFixed(6))))} />
        <output htmlFor={`${id}-${index}`}>{current} <span>{unit}</span></output>
        {index === 1 ? <div aria-hidden="true" className="creation-resource-scale"><span>{min} {unit}</span><span>{max} {unit}</span></div> : null}
      </div>
    })}
  </fieldset>
}
