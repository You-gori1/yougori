import { SharingPanel } from "@/components/sharing-panel"
import { pcDefinition, publicationDestination, publicationDestinations, sameEndpoint, type CapabilityEndpoint, type GraphEnvironment } from "@/components/graph-capabilities"
import type { useCapabilityConnections } from "@/components/use-capability-connections"
import { Button } from "@/components/ui/button"

export function FixedWorkspaceControl({ kind, side, wiring, environments }: { kind: "pc" | "public" | "local"; side: "top" | "bottom" | "left" | "right"; wiring: ReturnType<typeof useCapabilityConnections>; environments: GraphEnvironment[] }) {
  const definition = kind === "pc" ? pcDefinition : publicationDestinations.find(item => item.kind === kind)!
  const Icon = definition.icon
  const endpoint: CapabilityEndpoint = kind === "pc" ? { kind: "capability", id: "pc" } : { kind: "publication", id: kind }
  const highlighted = sameEndpoint(wiring.active, endpoint) || sameEndpoint(wiring.hovered, endpoint)
  const count = environments.reduce((total, env) => total + (kind === "pc" ? Number(Boolean(env.workspace?.shares.length)) : new Set(env.workspace?.publications.filter(p => publicationDestination(p.kind) === kind).map(p => p.port)).size), 0)
  const pointPosition = side === "top" ? "-top-px left-1/2 -translate-x-1/2 -translate-y-1/2" : side === "bottom" ? "-bottom-px left-1/2 -translate-x-1/2 translate-y-1/2" : side === "left" ? "top-1/2 -left-px -translate-x-1/2 -translate-y-1/2" : "top-1/2 -right-px translate-x-1/2 -translate-y-1/2"
  return <div className={`workspace-access-card relative min-w-0 ${side === "bottom" ? "w-64 shrink-0" : "w-full"}`} data-capability-card={kind === "pc" ? kind : undefined} data-publication-card={kind === "pc" ? undefined : kind}>
    <Button aria-label={definition.title} aria-pressed={sameEndpoint(wiring.active, endpoint)} className={`workspace-access-button w-full touch-none ${highlighted ? "border-primary! ring-2 ring-primary/20" : ""}`} onClick={() => wiring.clickEndpoint(endpoint)} onPointerDown={event => wiring.pointerDown(event, endpoint)} title={kind === "pc" ? "Connect to an environment to choose shared folders" : "Connect to an application's port"} variant="outline">
      <span className="workspace-access-icon"><Icon aria-hidden="true" /></span>
      <span>{kind === "public" ? "Public access" : definition.title}</span>
      {count ? <span className="workspace-access-count" aria-label={`${count} connected`}>{count}</span> : null}
    </Button>
    {kind === "pc" ? <div className="absolute right-1 top-1"><SharingPanel environmentId="my-pc" compact /></div> : null}
    <Button aria-label={`Connect ${definition.title}`} aria-pressed={sameEndpoint(wiring.active, endpoint)} className={`absolute! z-30 size-11! touch-none rounded-full! p-0! hover:bg-transparent ${pointPosition}`} data-capability-connection-point={kind === "pc" ? kind : undefined} data-publication-connection-point={kind === "pc" ? undefined : kind} data-connection-side={side} onClick={() => wiring.clickEndpoint(endpoint)} onPointerDown={event => wiring.pointerDown(event, endpoint)} variant="ghost"><span aria-hidden="true" className="pointer-events-none size-4 rounded-full border-[3px] border-background bg-primary shadow-[0_0_0_1px_var(--primary)]" /></Button>
  </div>
}
