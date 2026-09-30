import { CpuIcon, GlobeIcon, HardDriveIcon, LaptopIcon, MemoryStickIcon, NetworkIcon, ShieldCheckIcon } from "lucide-react"
import { environmentKindLabel } from "@/lib/domain"
import type { EnvironmentKind } from "@/types/platform"

import { isolationPresentation } from "@/components/dialogs/environment-creation-options"

export function CreationSectionHeading({ number, title, description }: { number: string; title: string; description: string }) {
  return <div className="flex items-start gap-3">
    <span aria-hidden="true" className="flex size-7 shrink-0 items-center justify-center rounded-lg border bg-background font-mono text-[11px] text-muted-foreground">{number}</span>
    <div className="flex min-w-0 flex-col gap-1"><h2 className="text-sm font-semibold tracking-tight">{title}</h2><p className="text-xs leading-relaxed text-muted-foreground">{description}</p></div>
  </div>
}

const number = (value: number) => Number.isFinite(value) ? String(value) : "—"

export function EnvironmentCreationPreview({ kind, name, runtime, cpu, memory }: { kind: EnvironmentKind; name: string; runtime: string; cpu: number; memory: number }) {
  const Icon = isolationPresentation[kind].icon
  const source = kind === "container" ? runtime : runtime === "builtin:alpine" ? "Built-in Alpine image" : runtime.split(/[\\/]/).pop()
  return <section aria-label="Configuration summary" className="overflow-hidden rounded-2xl border bg-background">
    <div className="flex items-center justify-between border-b px-4 py-3"><span className="text-[10px] font-semibold tracking-[0.16em] text-muted-foreground uppercase">Your new workspace</span><span className="rounded-full border bg-muted/40 px-2 py-0.5 text-[10px] text-muted-foreground">Draft</span></div>
    <div className="relative isolate overflow-hidden px-4 py-5">
      <div aria-hidden="true" className="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(var(--border)_1px,transparent_1px)] bg-size-[14px_14px] opacity-50" />
      <div className="flex items-center gap-3 rounded-xl border border-primary/20 bg-popover p-3 shadow-sm">
        <span className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary"><Icon aria-hidden="true" className="size-5" /></span>
        <div className="flex min-w-0 flex-col gap-1"><p className="truncate text-sm font-semibold" title={name.trim() || "Untitled environment"}>{name.trim() || "Untitled environment"}</p><p className="text-[11px] text-muted-foreground">{environmentKindLabel[kind]} workspace</p></div>
      </div>
      <div aria-hidden="true" className="mx-auto h-5 w-px border-l border-dashed border-primary/35" />
      <div className="mx-auto flex w-fit items-center gap-2 rounded-full border bg-popover px-3 py-1.5 text-[10px] text-muted-foreground"><HardDriveIcon aria-hidden="true" className="size-3" />Local storage · your PC</div>
    </div>
    <dl className="grid grid-cols-2 border-y bg-muted/15">
      <div className="flex flex-col gap-2 border-r p-4"><dt className="flex items-center gap-1.5 text-[11px] text-muted-foreground"><CpuIcon aria-hidden="true" className="size-3.5" />Starts with</dt><dd className="font-mono text-xl font-medium tracking-tight">{number(cpu)} <span className="font-sans text-xs font-normal text-muted-foreground">CPUs</span></dd></div>
      <div className="flex flex-col gap-2 p-4"><dt className="flex items-center gap-1.5 text-[11px] text-muted-foreground"><MemoryStickIcon aria-hidden="true" className="size-3.5" />Memory</dt><dd className="font-mono text-xl font-medium tracking-tight">{number(memory)} <span className="font-sans text-xs font-normal text-muted-foreground">GB</span></dd></div>
    </dl>
    <div className="flex min-w-0 flex-col gap-1.5 px-4 py-3"><span className="text-[10px] font-medium tracking-wider text-muted-foreground uppercase">Source</span><p className="truncate font-mono text-[11px]" title={runtime}>{source || "Select boot media"}</p></div>
  </section>
}

export function CreationConnectionsNote() {
  return <section aria-label="Connections after creation" className="flex flex-col gap-3 rounded-xl border border-dashed p-4">
    <div className="flex items-center gap-2"><NetworkIcon aria-hidden="true" className="size-4 text-primary" /><h3 className="text-xs font-semibold">Connect it your way</h3></div>
    <p className="text-xs leading-relaxed text-muted-foreground">After creation, connect capabilities to your node on the environment graph.</p>
    <div className="flex flex-wrap gap-1.5">{[{ icon: GlobeIcon, label: "Internet" }, { icon: LaptopIcon, label: "My PC" }].map(({ icon: Icon, label }) => <span key={label} className="inline-flex items-center gap-1.5 rounded-md border bg-background px-2 py-1 text-[10px] text-muted-foreground"><Icon aria-hidden="true" className="size-3" />{label}</span>)}</div>
    <p className="flex items-center gap-1.5 text-[10px] text-muted-foreground"><ShieldCheckIcon aria-hidden="true" className="size-3" />Both start disconnected.</p>
  </section>
}
