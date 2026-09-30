import { useState, type ReactNode } from "react"
import { workspaceApi } from "@/api/workspace-api"
import { cn } from "@/lib/utils"
import "./neocloud.css"

/** Main column, summary column and action bar. */
export function Workbench({ main, side, footer }: { main: ReactNode; side?: ReactNode; footer?: ReactNode }) {
  return <div className={cn("neo", !side && "is-single")} data-neocloud>
    <div className="neo-main">{main}</div>
    {side ? <aside className="neo-side" aria-label="Summary">{side}</aside> : null}
    {footer ? <div className="neo-footer">{footer}</div> : null}
  </div>
}

export function Heading({ kicker, title, children }: { kicker?: string; title: ReactNode; children?: ReactNode }) {
  return <header>
    {kicker ? <p className="neo-kicker">{kicker}</p> : null}
    <h2 className="neo-title">{title}</h2>
    {children ? <p className="neo-lede">{children}</p> : null}
  </header>
}

/** A link that opens in the system browser. */
export function WebLink({ url, children, className }: { url: string; children: ReactNode; className?: string }) {
  const [error, setError] = useState("")
  return <>
    <button type="button" className={cn("neo-link", className)} onClick={() => { void workspaceApi.openUrl(url).catch(e => setError(String(e))) }}>{children}</button>
    {error ? <span role="alert" className="neo-error">{error}</span> : null}
  </>
}

export function StepRail({ steps, current, reachable, onJump }: { steps: string[]; current: number; reachable(index: number): boolean; onJump(index: number): void }) {
  return <ol className="neo-steps" aria-label="Steps">
    {steps.map((step, i) => <li key={step} className={cn("neo-step", i < current && "is-done", i === current && "is-current")}>
      <button type="button" aria-current={i === current ? "step" : undefined} disabled={i === current || !reachable(i)} onClick={() => onJump(i)}>
        <span>{i + 1}. {step}</span>
      </button>
    </li>)}
  </ol>
}

export function Tile({ selected, disabled, onSelect, title, badge, children, meta, label, hero, className }: {
  selected?: boolean; disabled?: boolean; onSelect(): void; title: ReactNode; badge?: ReactNode; children?: ReactNode; meta?: ReactNode; label?: string; hero?: boolean; className?: string
}) {
  return <button type="button" aria-pressed={selected} aria-label={label} disabled={disabled} onClick={onSelect} className={cn("neo-tile", selected && "is-selected", hero && "is-hero", className)}>
    <span className="neo-tile-head"><span className="neo-tile-title">{title}</span>{badge}</span>
    {children ? <span className="neo-tile-text">{children}</span> : null}
    {meta ? <span className="neo-tile-meta">{meta}</span> : null}
  </button>
}

export function Badge({ tone, children }: { tone?: "primary" | "good" | "warn" | "bad"; children: ReactNode }) {
  return <span className={cn("neo-badge", tone && `is-${tone}`)}>{children}</span>
}

export function Pills<T extends string | number>({ label, value, options, onChange }: { label: string; value: T; options: readonly (readonly [T, string])[]; onChange(value: T): void }) {
  return <div className="neo-pills" role="group" aria-label={label}>
    {options.map(([id, text]) => <button key={String(id)} type="button" className="neo-pill" aria-pressed={value === id} onClick={() => onChange(id)}>{text}</button>)}
  </div>
}

export function Segments<T extends string>({ label, value, options, onChange }: { label: string; value: T; options: readonly { id: T; title: string; note: string }[]; onChange(value: T): void }) {
  return <div className="neo-segments" role="group" aria-label={label}>
    {options.map(o => <button key={o.id} type="button" className="neo-segment" aria-pressed={value === o.id} onClick={() => onChange(o.id)}>{o.title}<small>{o.note}</small></button>)}
  </div>
}

export function Field({ label, hint, error, children, className }: { label: ReactNode; hint?: ReactNode; error?: ReactNode; children: ReactNode; className?: string }) {
  return <label className={cn("neo-field", className)}>
    <span>{label}</span>
    {children}
    {error ? <span className="neo-error">{error}</span> : hint ? <small>{hint}</small> : null}
  </label>
}

/** Label and value rows; a missing value reads "Not chosen". */
export function Lines({ rows }: { rows: [string, ReactNode | null | undefined][] }) {
  return <dl className="neo-lines">
    {rows.map(([label, value]) => <div key={label} className={cn("neo-line", value == null && "is-empty")}><dt>{label}</dt><dd>{value ?? "Not chosen"}</dd></div>)}
  </dl>
}

/** Stock as a three-step meter. */
export function Meter({ level }: { level: 0 | 1 | 2 | 3 }) {
  return <span className="neo-meter" data-level={level} aria-hidden="true"><i /><i /><i /></span>
}
