import { SharingPanel } from "@/components/sharing-panel"
import { EnvironmentDuplicateAction, type DuplicateDestination } from "@/components/environment-duplicate-action"
import { EnvironmentNodeMenu } from "@/components/environment-node-menu"
import { DuplicateEnvironmentDialog } from "@/components/dialogs/duplicate-environment-dialog"
import { ConfigurationHelp } from "@/components/configuration-help"
import { EnvironmentList, EnvironmentListRow } from "@/components/environment-list"
import type { ReactNode } from "react"
import {
  Background,
  BackgroundVariant,
  ConnectionLineType,
  Handle,
  MarkerType,
  Position,
  ReactFlow,
  useEdgesState,
  useNodesState,
  type Connection as FlowConnection,
  type Edge,
  type BuiltInEdge,
  type Node,
  type NodeProps,
  type ReactFlowInstance,
} from "@xyflow/react"
import "@xyflow/react/dist/style.css"
import { EyeIcon, EyeOffIcon, TerminalIcon, GripVerticalIcon, ListIcon, NetworkIcon, MaximizeIcon, MinusIcon, PauseIcon, PlayIcon, PlusIcon, SquareIcon, SettingsIcon, XIcon } from "lucide-react"
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, type PointerEvent } from "react"
import { createPortal } from "react-dom"
import { allCapabilities, capabilityDefinitions, pcDefinition, capabilityEnabled, capabilityIssue, sameEndpoint, serviceId, type GraphEnvironment, type CapabilityEndpoint, type CapabilityKind } from "@/components/graph-capabilities"
import { useCapabilityConnections } from "@/components/use-capability-connections"
import { useWorkspaceFeatures } from "@/components/use-workspace-features"
import { useNodeFileDrop, fileDropIssue, type NodeFileCopy } from "@/components/use-node-file-drop"
import { neocloudApi } from "@/api/neocloud-api"
import { money, runpodApi, runpodExtra, stateLabel } from "@/api/runpod-api"
import { NodeFileCopyStatus } from "@/components/node-file-copy-status"
import { ModelNodeProgress } from "@/components/model-node-progress"
import { ModelWorkspace } from "@/components/model-workspace"
import { GraphWorkspaceDialogs } from "@/components/graph-workspace-dialogs"
import { FixedWorkspaceControl } from "@/components/graph-workspace-controls"
import { PublicAccessPresets } from "@/components/public-access-presets"
import { Status } from "@/components/shared/status"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { workspaceApi } from "@/api/workspace-api"
import { Spinner } from "@/components/ui/spinner"
import { Tooltip, TooltipPopup, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { usePlatform } from "@/context/platform-context"
import { formatBytesFromGb } from "@/lib/domain"
import { environmentLabel } from "@/lib/environment-category"
import { environmentActionLabel } from "@/lib/environment-actions"
import { graphColors } from "@/lib/graph-colors"
import { supportsConnections } from "@/lib/environment-connections"
import { useInstructionsTour } from "@/lib/instructions-tour"
import { tourPreviewEnvironment } from "@/lib/tour-preview"
import type { Environment } from "@/types/platform"
import "@/components/dashboard-actions.css"
import "@/components/environment-access.css"

interface EnvironmentNodeData extends Record<string, unknown> {
  fileHovered?: boolean
  fileCopy?: NodeFileCopy
  preview?: boolean
  accent: string
  active: CapabilityEndpoint | null
  hovered: CapabilityEndpoint | null
  pending: boolean
  environment: GraphEnvironment
  onEndpointClick(endpoint: CapabilityEndpoint): void
  onEndpointPointerDown(event: PointerEvent<HTMLElement>, endpoint: CapabilityEndpoint): void
  onCapabilityChange(environmentId: string, capability: CapabilityKind, enabled: boolean): void
  onConnect(environmentId: string): void
  onOpen(environmentId: string): void
  onSelect(environmentId: string): void
  onService(environmentId: string, port: number): void
  onDuplicate(environmentId: string, destination: DuplicateDestination): void
  onShares(environmentId: string): void
}

type EnvironmentNode = Node<EnvironmentNodeData, "environment">

const environmentSourceHandle = "environment-source"
const environmentTargetHandle = "environment-target"
const networkHandleClass = "!size-4 !border-[3px] !border-background !bg-[var(--node-accent)] after:absolute after:-inset-3 after:rounded-full hover:!bg-primary focus-visible:!bg-primary"

function NodeAction({ label, children }: { label: string; children: React.ReactElement }) {
  return (
    <Tooltip>
      <TooltipTrigger render={children} />
      <TooltipPopup>{label}</TooltipPopup>
    </Tooltip>
  )
}

function EnvironmentGraphNode({ data, selected }: NodeProps<EnvironmentNode>) {
  return <EnvironmentNodeMenu environment={data.environment} disabled={data.preview}><EnvironmentCard data={data} selected={selected} /></EnvironmentNodeMenu>
}

function EnvironmentCard({ data, selected, list = false }: { data: EnvironmentNodeData; selected?: boolean; list?: boolean }) {
  const { state, refreshPlatform, setEnvironmentStatus, environmentActions } = usePlatform()
  const { environment } = data
  const action = environmentActions[environment.id]
  const busy = Boolean(action) || environment.status === "provisioning"
  const lifecyclePending = action === "starting" || action === "connecting" || action === "stopping" || action === "disconnecting"
  const cloud = environment.kind === "cloud"
  const neocloud = state?.neocloudDeployments?.[environment.id]
  const [neoBusy, setNeoBusy] = useState(false)
  const runpod = neocloud?.provider === "runpod" ? neocloud : undefined
  const runpodState = runpod ? stateLabel(runpod.state, runpod.product) : ""
  const podReady = runpod?.product === "pod" && runpodExtra(runpod).sshReady === true
  const neoStopped = /^(stopped|exited|paused|shutoff)$/i.test(neocloud?.state ?? "")
  const neoPending = /requested|pending|creating|starting|stopping/i.test(neocloud?.state ?? "")
  const neoPower = async () => {
    if (!neocloud || neoBusy || neoPending) return
    setNeoBusy(true)
    try { await (runpod ? runpodApi.action(environment.id, neoStopped ? "start" : "stop") : neocloudApi.action(environment.id, neoStopped ? "start" : "stop")) }
    catch { /* The provider error is persisted on the node and shown in configuration. */ }
    finally { await refreshPlatform().catch(() => undefined); setNeoBusy(false) }
  }
  const dropHint = fileDropIssue(environment) ?? (cloud ? "Drop to copy into this cloud account's home folder" : "Drop to copy here")
  const [addressVisible, setAddressVisible] = useState(false)
  const creating = environment.status === "provisioning"
  const isNativeApplication = environment.provider === "nativeSandbox" || environment.kind === "computerBranch"
  const canConnect = supportsConnections(environment) && neocloud?.product !== "serverless"
  const canPause = !list && !cloud && environment.status === "running" && environment.provider !== "nativeSandbox" && environment.kind !== "computerBranch"
  const endpoint: CapabilityEndpoint = { kind: "environment", id: environment.id }
  const issue = data.active?.kind === "capability" ? capabilityIssue(environment, data.active.id) : null
  const eligible = data.active?.kind === "capability" && !issue && !data.pending
  const highlighted = sameEndpoint(data.active, endpoint) || (eligible && sameEndpoint(data.hovered, endpoint))
  const enabledCapabilities = allCapabilities.filter(({ capability }) => capabilityEnabled(environment, capability))
  const running = environment.status === "running"
  const needsNeoSsh = Boolean(neocloud && neocloud.provider !== "runpod" && neocloud.product !== "serverless" && environment.runtime.startsWith("Neocloud ·"))
  // A RunPod pod opens once Yougori has reached it over SSH; until then its details explain the wait.
  const runpodWaiting = Boolean(runpod && runpod.product === "pod" && !podReady && !running)
  const launchLabel = runpod ? (runpod.product === "serverless" ? "API" : running || podReady ? "Open" : runpodState === "Stopped" ? "Details" : runpodState === "Deleted" ? "Details" : "Starting…") : neocloud?.product === "serverless" ? "Model API" : needsNeoSsh ? "Set up SSH" : cloud ? "Open" : running ? "Stop" : environment.status === "paused" ? "Resume" : "Start"
  const configureOnDoubleClick = (event: React.MouseEvent<HTMLElement>) => {
    if (data.preview || (event.target as HTMLElement).closest("button, a, input, label, [role=switch], .react-flow__handle")) return
    data.onSelect(environment.id)
  }

  const statusSummary = action ? <span className="shrink-0 text-[10px] text-muted-foreground" role="status">{environmentActionLabel[action]}</span> : environment.status === "error" ? <span className="environment-needs-attention">Needs attention</span> : runpod ? <span className="text-[11px] text-muted-foreground">{running ? "Connected" : runpodState}{runpod.product === "pod" && runpodState === "Running" && runpodExtra(runpod).hourlyUsd ? ` · ${money(runpodExtra(runpod).hourlyUsd)}/hr` : ""}</span> : neocloud?.product === "serverless" ? <span className="text-[11px] text-muted-foreground">{neocloud.state}</span> : cloud ? <span className="text-[11px] text-muted-foreground">{environment.status === "running" ? "Connected" : "Disconnected"}</span> : <Status compact status={environment.status} />
  const services = !cloud && environment.workspace?.services.length ? <div aria-label={`Services in ${environment.name}`} className="nodrag flex flex-wrap gap-x-3 gap-y-4 border-b px-3 pb-2 pt-3">
        {environment.workspace.services.map(service => {
          const endpoint: CapabilityEndpoint = { kind: "service", id: serviceId(environment.id, service.port) }
          const publications = environment.workspace!.publications.filter(p => p.port === service.port)
          const highlighted = sameEndpoint(data.active, endpoint) || sameEndpoint(data.hovered, endpoint)
          return <div className="relative min-w-14" data-service-card={endpoint.id} key={service.port}>
            <Button aria-label={`Port ${service.port} in ${environment.name}`} className={`h-6! w-full text-[10px]! ${highlighted ? "ring-2 ring-primary" : ""}`} onClick={() => data.onService(environment.id, service.port)} size="xs" title={`${service.name} · ${service.protocol.toUpperCase()} ${service.port}`} variant="outline">:{service.port}</Button>
            {!list ? <Button aria-label={`Connect port ${service.port} in ${environment.name}`} aria-pressed={sameEndpoint(data.active, endpoint)} className="absolute! -top-3 left-1/2 z-30 size-11! touch-none rounded-full! p-0! hover:bg-transparent" data-service-connection-point={endpoint.id} data-connection-side="top" onClick={() => data.onEndpointClick(endpoint)} onPointerDown={event => data.onEndpointPointerDown(event, endpoint)} style={{ transform: "translate(-50%, -50%) scale(var(--connector-scale, 1))" }} variant="ghost"><span aria-hidden="true" style={{ backgroundColor: data.accent }} className="pointer-events-none size-3 rounded-full border-[3px] border-background bg-primary shadow-[0_0_0_1px_var(--node-accent)]" /></Button> : null}
            {!list && publications.length ? <button aria-label={`Published destinations for port ${service.port}`} className="mt-0.5 block w-full text-center text-[8px] leading-3 text-primary" onClick={() => data.onService(environment.id, service.port)} title={publications.map(p => `${p.kind}: ${p.urls.join(", ")}`).join("\n")} type="button">{publications.map(p => p.kind === "cloudflare" ? "CF" : p.kind === "local" ? "LAN" : "Public").join(" · ")}</button> : null}
          </div>
        })}
      </div> : null
  const cloudAddress = environment.runtime.startsWith("shared://") ? <span className="text-[11px] text-muted-foreground">Hosted remotely</span> : cloud ? <div className={`nodrag flex min-w-0 items-center gap-1 text-[11px] font-normal text-muted-foreground ${list ? "w-48 shrink-0" : "mt-2"}`}>
    <span className={`min-w-0 flex-1 truncate ${addressVisible ? "" : "text-[20px] leading-none tracking-wide"}`}>{addressVisible ? environment.runtime : "••••••@••••••"}</span>
    <Button aria-label={addressVisible ? "Hide cloud address" : "Show cloud address"} aria-pressed={addressVisible} onClick={() => setAddressVisible(value => !value)} size="icon-xs" type="button" variant="ghost" className="shrink-0">
      {addressVisible ? <EyeOffIcon aria-hidden="true" /> : <EyeIcon aria-hidden="true" />}
    </Button>
  </div> : null
  const access = list && !cloud && !isNativeApplication ? <div className="nodrag environment-list-access-controls">
    {allCapabilities.map(({ capability }) => <label key={capability} title={capabilityIssue(environment, capability) ?? (capability === "pc" ? "Turn on to choose shared folders; turn off to disconnect them." : "Allow internet access")}>
      <Switch aria-label={`${capability === "internet" ? "Internet access" : "My PC access"} for ${environment.name}`} checked={capabilityEnabled(environment, capability)} disabled={busy || data.pending || Boolean(capabilityIssue(environment, capability))} onCheckedChange={enabled => data.onCapabilityChange(environment.id, capability, enabled)} />
      <span aria-hidden="true">{capability === "internet" ? "Internet" : "My PC"}{capability === "pc" && environment.workspace?.shares.length ? ` · ${environment.workspace.shares.length}` : ""}</span>
    </label>)}
  </div> : null
  const network = list && !cloud && !isNativeApplication ? <NodeAction label="Localhost, local network and public access for this environment's ports"><Button aria-label={`Network for ${environment.name}`} className="node-chat" onClick={() => data.onService(environment.id, environment.workspace?.services[0]?.port ?? 0)} size="xs" type="button" variant="ghost">Network{environment.workspace?.publications.length ? ` · ${environment.workspace.publications.length}` : ""}</Button></NodeAction> : null
  const actions = (<div className="nodrag environment-card-actions flex items-center gap-1 border-t px-3 py-2 [&_button]:z-40">
        {!list && !isNativeApplication && !cloud ? <NodeAction label={environment.workspace?.notice ? "Needs attention" : "Add a service port. Listening ports also appear automatically."}><Button data-tour="node-port" aria-label={`Add service port to ${environment.name}`} className="px-1.5 text-[10px]! tracking-wide" onClick={() => data.onService(environment.id, 0)} size="xs" variant="ghost">PORT</Button></NodeAction> : null}
        {!cloud ? <EnvironmentDuplicateAction name={environment.name} disabled={busy || data.preview} onSelect={destination => data.onDuplicate(environment.id, destination)} /> : null}
        <NodeAction label="Configuration">
          <Button aria-label={`Configuration for ${environment.name}`} className="shrink-0" onClick={() => data.onSelect(environment.id)} size="icon-xs" type="button" variant="ghost"><SettingsIcon aria-hidden="true" /></Button>
        </NodeAction>
        {!environment.runtime.startsWith("shared://") && !data.preview ? <SharingPanel environmentId={environment.id} compact /> : null}
        {cloud && !environment.runtime.startsWith("shared://") ? <EnvironmentDuplicateAction name={environment.name} disabled={busy || data.preview} onSelect={destination => data.onDuplicate(environment.id, destination)} /> : null}
        {!list && enabledCapabilities.length ? (
          <div aria-label="Attached capabilities" className="nodrag flex shrink-0 items-center gap-1">
            {enabledCapabilities.map(({ capability, title, icon: Icon }) => (
              <button
                aria-label={`Detach ${title} from ${environment.name}`}
                className="inline-flex h-5 w-6 shrink-0 items-center justify-center rounded border bg-muted/50 text-muted-foreground outline-none hover:border-primary/40 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
                disabled={data.pending}
                key={capability}
                onClick={() => capability === "pc" ? data.onShares(environment.id) : data.onCapabilityChange(environment.id, capability, false)}
                title={capability === "pc" ? "Manage shared folders" : `Click to detach ${title}`}
                type="button"
              >
                <Icon aria-hidden="true" className="size-3.5" />
              </button>
            ))}
          </div>
        ) : null}
        {canPause ? (
          <NodeAction label={`Pause ${environment.name}`}>
            <Button aria-label={`Pause ${environment.name}`} disabled={busy} loading={action === "pausing"} onClick={() => void setEnvironmentStatus(environment.id, "paused").catch(() => undefined)} size="icon-xs" type="button" variant="ghost"><PauseIcon aria-hidden="true" /></Button>
          </NodeAction>
        ) : null}
        {neocloud && neocloud.state !== "Deleted" && (neocloud.resourceId || (neocloud.product === "serverless" && neoStopped)) ? <NodeAction label={runpod ? (runpod.product === "serverless" ? (neoStopped ? `Resume ${environment.name}` : `Pause ${environment.name}; it keeps its API address`) : neoStopped ? `Start ${environment.name}` : `Stop ${environment.name}. Its volume is kept and billed monthly.`) : neoStopped ? `Start ${environment.name} at ${neocloud.provider}` : neocloud.provider === "civo" ? `Power off ${environment.name}. Civo continues charging for stopped instances.` : `Stop ${environment.name} at ${neocloud.provider}. Storage charges may continue.`}><Button aria-label={`${neoStopped ? "Start" : "Stop"} provider compute for ${environment.name}`} disabled={busy || neoBusy || neoPending} loading={neoBusy} onClick={() => void neoPower()} size="xs" type="button" variant="outline">{runpod ? (runpod.product === "serverless" ? (neoStopped ? "Resume" : "Pause") : neoStopped ? "Start pod" : "Stop pod") : neocloud.product === "serverless" ? neoStopped ? "Create endpoint" : "Stop endpoint" : neoStopped ? "Start compute" : neocloud.provider === "civo" ? "Power off" : "Stop compute"}</Button></NodeAction> : null}
        {running && !cloud && !isNativeApplication ? (
          <NodeAction label={`Open ${environment.name}`}>
            <Button data-tour="node-open" aria-label="Open" disabled={busy} loading={action === "opening"} onClick={() => data.onOpen(environment.id)} size="icon-xs" type="button" variant="ghost"><TerminalIcon aria-hidden="true" /></Button>
          </NodeAction>
        ) : null}
        <Button data-tour="node-launch" className="node-launch ml-auto" data-running={running ? "true" : undefined} aria-label={list && !creating && !isNativeApplication && ["Start", "Resume", "Stop"].includes(launchLabel) ? launchLabel : undefined} aria-busy={creating || lifecyclePending || (cloud && action === "opening") || undefined} disabled={isNativeApplication || busy} loading={lifecyclePending || (cloud && action === "opening")} title={creating ? "Creating this environment. You can keep using Yougori." : action ? environmentActionLabel[action] : undefined} onClick={() => needsNeoSsh || neocloud?.product === "serverless" || runpodWaiting ? data.onSelect(environment.id) : !cloud && running ? void setEnvironmentStatus(environment.id, "stopped").catch(() => undefined) : data.onOpen(environment.id)} size="xs" type="button" variant={running ? "outline" : "default"}>
          {creating ? <><Spinner aria-hidden="true" />Creating…</> : isNativeApplication ? "Unavailable" : list && (launchLabel === "Start" || launchLabel === "Resume") ? <PlayIcon aria-hidden="true" /> : list && launchLabel === "Stop" ? <SquareIcon aria-hidden="true" /> : launchLabel}
        </Button>
        {!data.preview && !creating && environment.description.startsWith("Hugging Face · ") ? <ModelWorkspace environmentId={environment.id} compact /> : null}
      </div>)

  if (list) return <EnvironmentListRow
    environment={environment}
    accent={data.accent}
    canLink={canConnect}
    busy={Boolean(data.pending || busy || data.fileCopy?.busy)}
    fileHovered={data.fileHovered}
    fileStatus={<NodeFileCopyStatus copy={data.fileCopy} />}
    dropHint={dropHint}
    status={statusSummary}
    access={access}
    services={services || network || cloudAddress ? <div className="environment-list-services-controls">{services}{network}{cloudAddress}</div> : null}
    actions={actions}
    onConfigure={() => data.onSelect(environment.id)}
    onDoubleClick={configureOnDoubleClick}
  />

  return (
    <article
      data-environment-id={environment.id}
      onDoubleClick={configureOnDoubleClick}
      data-tour-preview={data.preview || undefined}
      data-file-drop-target={data.fileHovered || undefined}
      onClickCapture={data.preview ? event => { event.preventDefault(); event.stopPropagation() } : undefined}
      onPointerDownCapture={data.preview ? event => { event.preventDefault(); event.stopPropagation() } : undefined}
      data-environment-color={data.accent}
      data-remote-kind={environment.runtime.startsWith("shared://") ? "shared" : cloud ? "cloud" : undefined}
      style={{ "--node-accent": data.accent } as React.CSSProperties}
      data-selected={selected || highlighted || undefined}
      aria-busy={Boolean(data.pending || busy || data.fileCopy?.busy)}
      data-connection-eligible={data.active?.kind === "capability" ? eligible : undefined}
      title={issue ?? undefined}
      className={`${list ? "workspace-list-row" : "workspace-node"} relative rounded-lg border bg-background shadow-sm/5 transition-colors ${highlighted ? "border-primary ring-2 ring-primary/30" : eligible || selected ? "border-primary/70 ring-2 ring-primary/10" : "border-border"}`}
    >
      {data.fileHovered ? <div className="pointer-events-none absolute inset-0 z-50 flex items-center justify-center rounded-lg border-2 border-dashed border-primary bg-background/95 p-4 text-center text-xs" role="status">
        <span>{data.fileCopy?.busy ? "A copy is already in progress" : dropHint}<span className="mt-2 block text-[10px] text-muted-foreground">Original files and folders stay on your computer.</span></span>
      </div> : null}
      {!list && canConnect ? <Handle aria-label={`Connect another environment to ${environment.name}`} className={networkHandleClass} id={environmentTargetHandle} position={Position.Left} type="target" isConnectable={!data.preview && !data.active} /> : null}
      {services}
      <div className="p-3 environment-card-body">
        <div className="flex items-start gap-2">
          <span className="cursor-grab pt-0.5 text-muted-foreground active:cursor-grabbing" title="Move environment" data-node-drag-grip><GripVerticalIcon aria-hidden="true" className="size-4" /></span>
          <button
            aria-label={`Configure ${environment.name}`}
            data-tour="node-configure"
            className="nodrag block min-w-0 flex-1 text-left outline-none focus-visible:rounded-md focus-visible:ring-2 focus-visible:ring-ring"
            onClick={() => data.onSelect(environment.id)}
            type="button"
          >
            <span className="flex items-center justify-between gap-3">
              <span className="truncate text-sm font-medium">{environment.name}</span>
              {statusSummary}
            </span>
            <span className="mt-1 block truncate text-[11px] text-muted-foreground">{data.preview ? "Preview only · no resources used" : environmentLabel(environment)}</span>
          </button>
        </div>
        {cloud ? <div className="mt-3 border-t pt-1">{cloudAddress}</div> : <dl className="mt-3 grid grid-cols-3 gap-3 border-t pt-2 text-[10px] tabular-nums text-muted-foreground">
          <div><dt>CPU</dt><dd className="mt-1 text-xs text-foreground">{Math.round(environment.cpuUsage)}%</dd></div>
          <div><dt>Memory</dt><dd className="mt-1 text-xs text-foreground">{environment.memoryUsageGb.toFixed(1)} GB</dd></div>
          <div><dt>Storage</dt><dd className="mt-1 text-xs text-foreground">+{formatBytesFromGb(environment.storageDeltaGb)}</dd></div>
        </dl>}

        <NodeFileCopyStatus copy={data.fileCopy} />
        {!data.preview && environment.description.startsWith("Hugging Face · ") ? <ModelNodeProgress environment={environment} /> : null}
      </div>
      {actions}
      {!list && canConnect ? <Handle data-tour="node-connect" aria-label={`Connect ${environment.name} to another environment`} className={networkHandleClass} id={environmentSourceHandle} position={Position.Right} type="source" isConnectable={!data.preview && !data.active} /> : null}
      {!list && !cloud ? <Button
        aria-label={`Connect capabilities to ${environment.name}`}
        aria-pressed={sameEndpoint(data.active, endpoint)}
        className="nodrag nopan absolute! -bottom-px left-1/2 z-30 size-11! touch-none rounded-full! p-0! hover:bg-transparent"
        data-environment-connection-point={environment.id}
        loading={data.pending}
        onClick={() => data.onEndpointClick(endpoint)}
        onPointerDown={event => data.onEndpointPointerDown(event, endpoint)}
        style={{ transform: "translate(-50%, 50%) scale(var(--connector-scale, 1))" }}
        title={issue ?? "Drag to a capability, or click to connect"}
        type="button"
        variant="ghost"
      >
        <span aria-hidden="true" style={{ backgroundColor: data.accent }} className={`pointer-events-none size-4 rounded-full border-[3px] border-background bg-primary shadow-[0_0_0_1px_var(--node-accent)] ${data.pending ? "invisible" : highlighted ? "ring-4 ring-primary/20" : ""}`} />
      </Button> : null}
    </article>
  )
}

const HostCliView = lazy(() => import("@/components/host-cli-view"))

const nodeTypes = { environment: EnvironmentGraphNode }

const layoutStorageKey = "yougori.environment-node-positions.v1"
function readNodePositions(): Map<string, { x: number; y: number }> {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(layoutStorageKey) ?? "{}")
    if (!saved || typeof saved !== "object" || Array.isArray(saved)) return new Map()
    return new Map(Object.entries(saved).filter((entry): entry is [string, { x: number; y: number }] => {
      const position = entry[1]
      return Boolean(position && typeof position === "object" && Number.isFinite(position.x) && Number.isFinite(position.y))
    }))
  } catch { return new Map() }
}

export function EnvironmentGraph({ environments, connections, errorContainer, onConnect, onOpen, onSelect, footer }: {
  footer?: (openEdit: () => void, editDisabled: boolean, editActive: boolean) => ReactNode
  errorContainer: HTMLElement | null
  environments: Environment[]
  connections: Array<{ id: string; sourceId: string; targetId: string; direction: "oneWay" | "bidirectional"; active: boolean; enforcementStatus?: "enforced" | "pending" | "error" }>
  onConnect(sourceId: string, targetId?: string): void
  onOpen(environmentId: string): void
  onSelect(environmentId: string): void
}) {
  const { updateContainerNetwork } = usePlatform()
  const [savedPositions] = useState(readNodePositions)
  const [duplicate, setDuplicate] = useState<{ environmentId: string; destination: DuplicateDestination } | null>(null)
  const openDuplicate = useCallback((environmentId: string, destination: DuplicateDestination) => setDuplicate({ environmentId, destination }), [])
  const duplicateSource = environments.find(environment => environment.id === duplicate?.environmentId)
  const tour = useInstructionsTour()
  const [preferredView, setPreferredView] = useState<"nodes" | "list" | "cli" | "edit">(() => {
    try { const saved = localStorage.getItem("yougori.workspace-view"); return saved === "nodes" || saved === "cli" || saved === "edit" ? saved : "list" } catch { return "list" }
  })
  const [cliMounted, setCliMounted] = useState(preferredView === "cli")
  const [editMounted, setEditMounted] = useState(preferredView === "edit")
  const previousView = useRef<"nodes" | "list">(preferredView === "list" ? "list" : "nodes")
  const view = tour?.active ? "nodes" : preferredView
  const preview = useMemo(() => tourPreviewEnvironment(tour), [tour])
  const graphContainerRef = useRef<HTMLDivElement>(null)
  const fileDrop = useNodeFileDrop(graphContainerRef, environments)
  const flowRef = useRef<ReactFlowInstance<EnvironmentNode, BuiltInEdge> | null>(null)
  const workspace = useWorkspaceFeatures(environments)
  const { decorated, openShares, openService, connectPublication, refresh } = workspace
  const colors = useMemo(() => {
    const accents = graphColors(environments.map(environment => environment.id))
    for (const environment of environments) {
      if (environment.runtime.startsWith("shared://")) accents[environment.id] = "#00B7CD"
      else if (environment.kind === "cloud") accents[environment.id] = "#FF9100"
    }
    return accents
  }, [environments])

  const setCapability = useCallback(async (environmentId: string, capability: CapabilityKind, enabled: boolean) => {
    const environment = environments.find((item) => item.id === environmentId)
    if (!environment) throw new Error("Environment not found")
    if (capability === "pc") {
      if (enabled) { openShares(environmentId); return }
      try {
        const { shares } = await workspaceApi.services(environmentId)
        for (const share of shares) await workspaceApi.unshare(share.id)
      } finally {
        await refresh(environmentId)
      }
      return
    }
    if (capability === "internet") return updateContainerNetwork(environmentId, enabled)
  }, [environments, openShares, refresh, updateContainerNetwork])

  const wiring = useCapabilityConnections(graphContainerRef, decorated, setCapability, connectPublication)
  const { active, hovered, pending, change, clickEndpoint, pointerDown, scheduleGeometry, cancel } = wiring

  const selectView = useCallback((mode: "nodes" | "list" | "cli" | "edit") => {
    cancel()
    if (mode === "cli") setCliMounted(true)
    else if (mode === "edit") setEditMounted(true)
    else previousView.current = mode
    setPreferredView(mode)
    try { localStorage.setItem("yougori.workspace-view", mode) } catch { /* Storage may be unavailable. */ }
  }, [cancel])
  const hideCli = useCallback(() => {
    selectView(previousView.current)
    document.getElementById("host-terminal-toggle")?.focus()
  }, [selectView])
  const hideEditor = useCallback(() => {
    selectView(previousView.current)
    document.getElementById("edit-app-toggle")?.focus()
  }, [selectView])
  useEffect(() => {
    const shortcut = (event: KeyboardEvent) => {
      if (!tour?.active && event.ctrlKey && !event.altKey && !event.metaKey && event.code === "Backquote" && !event.repeat) {
        event.preventDefault()
        selectView(preferredView === "cli" ? previousView.current : "cli")
      }
    }
    window.addEventListener("keydown", shortcut)
    return () => window.removeEventListener("keydown", shortcut)
  }, [preferredView, selectView, tour])

  const initialNodes = useMemo<EnvironmentNode[]>(() => [...(preview ? [preview] : []), ...decorated].map((environment, index): EnvironmentNode => ({
      id: environment.id,
      type: "environment",
      draggable: environment === preview ? false : undefined,
      selectable: environment === preview ? false : undefined,
      connectable: environment === preview ? false : undefined,
      position: (environment !== preview && savedPositions.get(environment.id)) || { x: 70 + (index % 3) * 360, y: 65 + Math.floor(index / 3) * 310 },
      data: { fileHovered: fileDrop.hovered === environment.id, fileCopy: fileDrop.copies[environment.id], preview: environment === preview, accent: colors[environment.id] ?? "#7194c2", active, hovered, pending: pending.has(environment.id), environment, onCapabilityChange: change, onEndpointClick: clickEndpoint, onEndpointPointerDown: pointerDown, onConnect, onOpen, onSelect, onService: openService, onShares: openShares, onDuplicate: openDuplicate },
    })), [savedPositions, fileDrop.hovered, fileDrop.copies, preview, colors, active, hovered, pending, change, clickEndpoint, pointerDown, decorated, onConnect, onOpen, onSelect, openService, openShares, openDuplicate])
  const initialEdges = useMemo<BuiltInEdge[]>(() => connections.map((connection, index): BuiltInEdge => ({
      id: connection.id,
      type: "smoothstep",
      pathOptions: { borderRadius: 14, offset: 24 + index * 8, stepPosition: 0.35 + index / Math.max(1, connections.length) * 0.3 },
      source: connection.sourceId,
      sourceHandle: environmentSourceHandle,
      target: connection.targetId,
      targetHandle: environmentTargetHandle,
      deletable: false,
      label: connection.active && connection.enforcementStatus === "error" ? "Needs attention" : connection.active && connection.enforcementStatus === "pending" ? "Pending" : undefined,
      labelStyle: { fill: "var(--muted-foreground)", fontSize: 11 },
      labelBgStyle: { fill: "var(--background)" },
      markerEnd: { type: MarkerType.ArrowClosed, width: 14, height: 14, color: colors[connection.sourceId] },
      markerStart: connection.direction === "bidirectional" ? { type: MarkerType.ArrowClosed, width: 14, height: 14, color: colors[connection.sourceId] } : undefined,
      style: { stroke: colors[connection.sourceId], strokeWidth: connection.active ? 1.8 : 1.4, opacity: connection.active ? 0.9 : 0.4, strokeDasharray: connection.active && (!connection.enforcementStatus || connection.enforcementStatus === "enforced") ? undefined : "5 4" },
    })), [colors, connections])
  const [nodes, setNodes, onNodesChange] = useNodesState<EnvironmentNode>(initialNodes)
  const [edges, setEdges, onEdgesChange] = useEdgesState(initialEdges)
  const knownNodesRef = useRef(new Set(initialNodes.map(node => node.id)))
  const fitNewNodesRef = useRef(false)

  useEffect(() => {
    if (initialNodes.some(node => !knownNodesRef.current.has(node.id))) fitNewNodesRef.current = true
    knownNodesRef.current = new Set(initialNodes.map(node => node.id))
    setNodes((current) => {
      const previous = new Map(current.map(node => [node.id, node]))
      return initialNodes.map(node => {
        const existing = previous.get(node.id)
        // Preserve selection, drag state and measurements when telemetry updates.
        if (existing) return { ...existing, data: node.data }
        if (savedPositions.has(node.id) && !node.data.preview) return node
        let position = node.position
        while (current.some(item => Math.abs(item.position.x - position.x) < 310 && Math.abs(item.position.y - position.y) < 280)) {
          position = { ...position, y: position.y + 310 }
        }
        return { ...node, position }
      })
    })
  }, [initialNodes, savedPositions, setNodes])
  useEffect(() => setEdges(initialEdges), [initialEdges, setEdges])
  useEffect(() => {
    // setNodes above is asynchronous. The previous, already measured nodes can
    // still be rendered here while a new node (including the tour preview) is
    // pending. Keep the fit request until every expected node is measured.
    const allMeasured = nodes.length === knownNodesRef.current.size && nodes.every(node => knownNodesRef.current.has(node.id) && node.measured?.width && node.measured?.height)
    if (fitNewNodesRef.current && flowRef.current && nodes.length && allMeasured) {
      fitNewNodesRef.current = false
      void flowRef.current.fitView({ padding: 0.25, maxZoom: 1, nodes: preview ? [{ id: preview.id }] : undefined })
    }
    scheduleGeometry()
  }, [nodes, preview, scheduleGeometry])

  const validConnection = useCallback((connection: FlowConnection | Edge) => {
    const environment = environments.find((item) => item.id === connection.target)
    return connection.sourceHandle === environmentSourceHandle
      && connection.targetHandle === environmentTargetHandle
      && connection.source !== connection.target
      && environments.some((item) => item.id === connection.source && supportsConnections(item))
      && Boolean(environment && supportsConnections(environment))
  }, [environments])

  const connect = useCallback((connection: FlowConnection) => {
    if (!connection.source || !connection.target || !validConnection(connection)) return
    onConnect(connection.source, connection.target)
  }, [onConnect, validConnection])

  return (
    <TooltipProvider delay={0}>
      {wiring.feedback && errorContainer ? createPortal(
        <div className="flex items-center gap-2 rounded-md border border-destructive/30 bg-background px-2 py-0.5 text-xs text-destructive-foreground" role="alert">
          <p className="min-w-0 flex-1 truncate" title={wiring.feedback}>{wiring.feedback}</p>
          <Button aria-label="Dismiss graph error" onClick={wiring.dismissFeedback} size="icon-xs" type="button" variant="ghost"><XIcon aria-hidden="true" /></Button>
        </div>, errorContainer,
      ) : null}
      <div
        className="workspace-graph relative isolate w-full overflow-hidden rounded-lg border bg-background"
        data-environment-graph
        data-view={view}
        data-connecting={Boolean(active)}
        onClickCapture={event => {
          if (wiring.consumeClick()) {
            event.preventDefault()
            event.stopPropagation()
            return
          }
          if (active?.kind === "publication") {
            const service = (event.target as HTMLElement).closest<HTMLElement>("[data-service-card]")
            if (service) { event.preventDefault(); event.stopPropagation(); clickEndpoint({ kind: "service", id: service.dataset.serviceCard! }) }
            return
          }
          if (active?.kind !== "capability") return
          const node = (event.target as HTMLElement).closest<HTMLElement>("[data-environment-id]")
          if (!node) return
          event.preventDefault()
          event.stopPropagation()
          clickEndpoint({ kind: "environment", id: node.dataset.environmentId! })
        }}
        onPointerCancel={wiring.cancel}
        onLostPointerCapture={event => { if (event.buttons) wiring.cancel() }}
        onPointerMove={wiring.pointerMove}
        onPointerUp={wiring.pointerUp}
        ref={graphContainerRef}
      >
        <div className="workspace-view-toolbar">
          <div className="flex items-center gap-2">
            <span>Environments</span>
            {view !== "nodes" ? <Button aria-label="Add or manage saved domain setups" size="xs" variant="ghost" onClick={() => window.dispatchEvent(new Event("yougori-open-public-presets-manager"))}>Domains</Button> : null}
          </div>
          <div role="group" aria-label="Environment view" className="workspace-view-switch">
            {(["list", "nodes", "cli"] as const).map(mode => <Button key={mode} data-instruction-view={mode} id={mode === "cli" ? "host-terminal-toggle" : undefined} size="xs" variant="ghost" aria-pressed={view === mode} aria-controls={mode === "cli" ? "host-terminal-panel" : undefined} title={mode === "cli" ? "CLI (Ctrl+`)" : undefined} disabled={Boolean(tour?.active)} onClick={() => selectView(mode)}>{mode === "nodes" ? <NetworkIcon aria-hidden="true" /> : mode === "list" ? <ListIcon aria-hidden="true" /> : <TerminalIcon aria-hidden="true" />}{mode === "nodes" ? "Nodes" : mode === "list" ? "List" : "CLI"}</Button>)}
          </div>
        </div>
        <div className="workspace-connection-dock workspace-connection-dock-top">
          <div aria-label="Scroll service destinations left or right" className="workspace-connection-scroll" data-service-destinations-scroll tabIndex={0}>
            <div className="workspace-connection-row">
              <section aria-label="Service destinations" className="workspace-publication-cards">
                <FixedWorkspaceControl kind="local" side="bottom" wiring={wiring} environments={decorated} />
                <FixedWorkspaceControl kind="public" side="bottom" wiring={wiring} environments={decorated} />
                <PublicAccessPresets environments={decorated} refresh={refresh} wiring={wiring} />
              </section>
              <span className="workspace-dock-divider" aria-hidden="true" />
              <Button className="workspace-domains-button" aria-label="Add or manage saved domain setups" size="sm" variant="ghost" onClick={() => window.dispatchEvent(new Event("yougori-open-public-presets-manager"))}><SettingsIcon aria-hidden="true" />Domains</Button>
            </div>
          </div>
        </div>
        <svg aria-hidden="true" className="pointer-events-none absolute inset-0 z-0 size-full" data-capability-lines>
          {wiring.lines.map(line => (
            <g key={line.id}>
              <path d={line.path} fill="none" stroke="var(--background)" strokeLinecap="round" strokeLinejoin="round" strokeWidth="6" />
              <path d={line.path} data-capability-line={line.id} data-inactive={line.inactive || undefined} fill="none" stroke={colors[line.environmentId]} strokeOpacity={line.inactive ? "0.45" : "0.85"} strokeDasharray={line.inactive ? "5 5" : undefined} strokeLinecap="round" strokeLinejoin="round" strokeWidth="2"><title>{line.inactive ? "Saved connection · node is off" : "Connection"}</title></path>
            </g>
          ))}
          {wiring.preview ? <path d={wiring.preview} data-connection-preview fill="none" stroke="var(--primary)" strokeDasharray="6 4" strokeLinecap="round" strokeWidth="2.5" /> : null}
        </svg>
        <div
          className="relative z-10 h-[clamp(460px,calc(100dvh-400px),1200px)]"
          data-environment-canvas
        >
          {cliMounted ? <div hidden={view !== "cli"} className="workspace-cli-view h-full min-h-0">
            <Suspense fallback={<div role="status" className="grid h-full place-items-center text-sm text-muted-foreground">Loading CLI…</div>}>
              <HostCliView visible={view === "cli"} onHide={hideCli} />
            </Suspense>
          </div> : null}
          {editMounted ? <div hidden={view !== "edit"} className="workspace-cli-view h-full min-h-0">
            <Suspense fallback={<div role="status" className="grid h-full place-items-center text-sm text-muted-foreground">Loading editor…</div>}>
              <HostCliView mode="edit" visible={view === "edit"} onHide={hideEditor} />
            </Suspense>
          </div> : null}
          {view === "cli" || view === "edit" ? null : view === "list" ? <EnvironmentList items={initialNodes} connections={connections} colors={colors} onConnect={onConnect}>
            {node => <EnvironmentCard key={node.id} data={node.data} list />}
          </EnvironmentList> : <ReactFlow
            connectionLineType={ConnectionLineType.SmoothStep}
            connectionRadius={32}
            edges={edges}
            fitView
            fitViewOptions={{ padding: 0.25, maxZoom: 1 }}
            maxZoom={1.5}
            minZoom={0.3}
            nodeDragThreshold={6}
            nodesDraggable={!active}
            nodes={nodes}
            nodeTypes={nodeTypes}
            onConnect={connect}
            onEdgeClick={(_, edge) => onConnect(edge.source, edge.target)}
            onEdgesChange={onEdgesChange}
            onInit={instance => { flowRef.current = instance; graphContainerRef.current?.style.setProperty("--connector-scale", String(1 / instance.getZoom())); scheduleGeometry() }}
            onMove={(_, viewport) => { graphContainerRef.current?.style.setProperty("--connector-scale", String(1 / viewport.zoom)); scheduleGeometry() }}
            onNodesChange={changes => {
              onNodesChange(changes)
              let changed = false
              for (const change of changes) {
                if (change.type === "position" && change.position && change.id !== preview?.id) {
                  savedPositions.set(change.id, { ...change.position })
                  changed = true
                }
              }
              if (changed) {
                try { localStorage.setItem(layoutStorageKey, JSON.stringify(Object.fromEntries(savedPositions))) } catch { /* Keep dragging available when storage is unavailable. */ }
              }
            }}
            onPaneClick={wiring.cancel}
            panOnDrag={!active}
            zoomOnDoubleClick={false}
            isValidConnection={validConnection}
            proOptions={{ hideAttribution: true }}
            style={{ height: "100%", width: "100%" }}
          >
            <Background color="var(--graph-dot)" gap={24} size={1.5} variant={BackgroundVariant.Dots} />
          </ReactFlow>}
          <div className="workspace-zoom-controls absolute right-3 top-3 z-20 flex gap-1 rounded-lg border bg-background p-1 shadow-xs">
            {active ? <Button aria-label="Cancel connection" onClick={wiring.cancel} size="icon-sm" title="Cancel connection (Esc)" variant="ghost"><XIcon aria-hidden="true" /></Button> : null}
            <Button aria-label="Zoom out" onClick={() => void flowRef.current?.zoomOut()} size="icon-sm" title="Zoom out" variant="ghost"><MinusIcon aria-hidden="true" /></Button>
            <Button aria-label="Zoom in" onClick={() => void flowRef.current?.zoomIn()} size="icon-sm" title="Zoom in" variant="ghost"><PlusIcon aria-hidden="true" /></Button>
            <Button aria-label="Fit environments" onClick={() => void flowRef.current?.fitView({ padding: 0.25, maxZoom: 1 })} size="icon-sm" title="Fit environments" variant="ghost"><MaximizeIcon aria-hidden="true" /></Button>
          </div>
          <span className="sr-only" aria-live="polite">{active ? active.kind === "capability" ? "Choose an environment. Press Escape to cancel." : "Choose a capability below. Press Escape to cancel." : ""}</span>
          {view !== "cli" && !environments.length && !preview ? <div className="workspace-empty pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-2 px-6 text-center"><p className="text-base font-medium">Your workspace starts here</p><p className="max-w-sm text-sm leading-6 text-muted-foreground">Choose New environment above to create a container, VM or MicroVM.</p><p className="mt-2 text-xs text-muted-foreground">Then connect its files, network and service ports here.</p></div> : null}
        </div>
        <div className="workspace-connection-dock">
          <div aria-label="Scroll connection controls left or right" className="workspace-connection-scroll" tabIndex={0}>
            <div className="workspace-connection-row">
              <section aria-label="Environment capabilities" className="workspace-capability-cards">
            {[pcDefinition, ...capabilityDefinitions].map(({ capability, icon: Icon, title }) => {
              const connectedCount = decorated.filter((environment) => capabilityEnabled(environment, capability)).length
              const endpoint: CapabilityEndpoint = { kind: "capability", id: capability }
              const armed = sameEndpoint(active, endpoint)
              const selectedEnvironment = active?.kind === "environment" ? environments.find(item => item.id === active.id) : null
              const issue = selectedEnvironment ? capabilityIssue(selectedEnvironment, capability) : null
              const eligible = Boolean(selectedEnvironment && !issue && !pending.has(selectedEnvironment.id))
              const highlighted = armed || (eligible && sameEndpoint(hovered, endpoint))
              return (
                <div className="workspace-access-card relative min-w-0" data-capability-card={capability} data-connection-eligible={selectedEnvironment ? eligible : undefined} key={capability}>
                <Button
                  aria-label={title}
                  aria-pressed={armed}
                  className={`workspace-access-button w-full touch-none ${highlighted ? "border-primary! ring-2 ring-primary/20" : eligible ? "border-primary/60" : ""}`}
                  data-capability-kind={capability}
                  onClick={() => clickEndpoint(endpoint)}
                  onPointerDown={event => pointerDown(event, endpoint)}
                  title={issue ?? `Connect ${title}`}
                  type="button"
                  variant={armed ? "secondary" : "outline"}
                >
                  <span className="workspace-access-icon"><Icon aria-hidden="true" /></span>
                  <span>{title}</span>
                  {connectedCount ? <span aria-label={`${connectedCount} connected`} className="workspace-access-count">{connectedCount}</span> : null}
                </Button>
                <Button
                  aria-label={`Connect ${title}`}
                  aria-pressed={armed}
                  className="absolute! -top-px left-1/2 z-30 size-11! -translate-x-1/2 -translate-y-1/2 touch-none rounded-full! p-0! hover:bg-transparent"
                  data-capability-connection-point={capability}
                  data-connection-side="top"
                  onClick={() => clickEndpoint(endpoint)}
                  onPointerDown={event => pointerDown(event, endpoint)}
                  title={issue ?? "Drag to an environment, or click to connect"}
                  type="button"
                  variant="ghost"
                >
                  <span aria-hidden="true" className={`pointer-events-none size-4 rounded-full border-[3px] border-background bg-primary shadow-[0_0_0_1px_var(--primary)] ${highlighted ? "ring-4 ring-primary/20" : ""}`} />
                </Button>
                </div>
              )
            })}
              </section>
            </div>
          </div>
        </div>

        <div className="workspace-graph-caption flex flex-wrap items-center justify-between gap-2 border-t px-4 py-2 text-[11px] text-muted-foreground">
          <span className="workspace-footer-counts"><span><strong>{environments.length}</strong> {environments.length === 1 ? "environment" : "environments"}</span><span aria-hidden="true">·</span><span><strong>{connections.length}</strong> {connections.length === 1 ? "connection" : "connections"}</span></span>
          {footer?.(() => selectView("edit"), Boolean(tour?.active), view === "edit")}
          <ConfigurationHelp label="Workspace controls">{view === "nodes" ? "Drag to arrange · Double-click a node to configure · Connect using the dots · Click a connection line to edit it" : view === "cli" ? "Ctrl+` toggles the CLI. Switch views to keep your terminal session running." : view === "edit" ? "Edit this app from a source checkout. Terminal sessions keep running when you switch views." : "Double-click a row to configure. Use the row controls to start, stop, or manage files and service ports."}</ConfigurationHelp>
        </div>
      </div>
      <GraphWorkspaceDialogs model={workspace} />
      {duplicate && duplicateSource ? <DuplicateEnvironmentDialog key={`${duplicate.environmentId}-${duplicate.destination}`} environment={duplicateSource} destination={duplicate.destination} onClose={() => setDuplicate(null)} /> : null}
    </TooltipProvider>
  )
}
