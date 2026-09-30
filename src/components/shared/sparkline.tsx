import { cn } from "@/lib/utils"

export function Sparkline({ values, className, label, unavailable = false }: { values: number[]; className?: string; label: string; unavailable?: boolean }) {
  const width = 180
  const height = 32
  const samples = unavailable ? [] : values.filter(Number.isFinite).slice(-120)
  const points = samples.map((value, index) => ({
    x: samples.length === 1 ? width - 3 : 3 + index / (samples.length - 1) * (width - 6),
    y: height - 3 - Math.min(100, Math.max(0, value)) / 100 * (height - 6),
  }))
  // Horizontal tangents keep curves inside the measured endpoints, without invented peaks.
  const path = points.map((point, index) => {
    const previous = points[index - 1]
    if (!previous) return `M ${point.x} ${point.y}`
    const middle = (previous.x + point.x) / 2
    return `C ${middle} ${previous.y}, ${middle} ${point.y}, ${point.x} ${point.y}`
  }).join(" ")
  const last = points.at(-1)

  return (
    <svg aria-label={label} className={cn("h-7 w-full text-primary", className)} preserveAspectRatio="none" role="img" viewBox={`0 0 ${width} ${height}`}>
      <title>{unavailable ? "Usage unavailable" : samples.length ? "Recent usage · fixed 0–100% scale" : "Waiting for usage samples"}</title>
      <line className="stroke-border" x1="0" x2={width} y1="16" y2="16" strokeDasharray="2 5" vectorEffect="non-scaling-stroke" />
      <line className="stroke-border" x1="0" x2={width} y1={height - 1} y2={height - 1} vectorEffect="non-scaling-stroke" />
      {last ? <>
        {points.length > 1 && points[0] ? <path d={`${path} L ${last.x} ${height - 1} L ${points[0].x} ${height - 1} Z`} fill="currentColor" fillOpacity="0.09" /> : null}
        <path d={path} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="1.8" vectorEffect="non-scaling-stroke" />
        <line x1={last.x} x2={last.x} y1={last.y} y2={last.y} stroke="currentColor" strokeLinecap="round" strokeWidth="4" vectorEffect="non-scaling-stroke" />
      </> : null}
    </svg>
  )
}
