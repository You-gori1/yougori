import template from "./connection-skill-template.txt?raw"
import { nodeConnections, supportsConnections } from "./environment-connections"
import type { Connection, Environment, PlatformState } from "@/types/platform"
import type { HostShare } from "@/api/workspace-api"

export interface SkillIssue { code: string; explanation: string; nextStep: string; side?: string; nodeId?: string }
export interface SkillPeer {
  connectionId: string; peerId: string; peerName: string; peerKind: Environment["kind"] | null
  peerStatus: Environment["status"] | "missing"; usableNow: boolean; summary: string
  issues: SkillIssue[]; limitations?: string[]; permissions: Connection["permissions"]; direction: Connection["direction"]
  remoteControl?: { method: string; port: number; canConnect: boolean; scope: string } | null
  commandsAllowedByPolicy?: boolean; canRunPeerCommandsNow?: boolean; commandApi?: string | null
  selectedFolders?: { ownerId: string; guestPath: string; apiPath: string }[]
}
export interface SkillSnapshot {
  generatedAt: string; sourceName: string; sourceStatus: Environment["status"]
  summary: { connectedNodes: number; connections: number; ready: number; blocked: number }
  connections: SkillPeer[]
}

export function readSkillSnapshot(text: string): SkillSnapshot {
  const json = text.replaceAll("\r\n", "\n").split("```json\n")[1]?.split("\n```")[0]
  if (!json) throw new Error("Skills is missing its current connection information. Refresh and try again.")
  const data = JSON.parse(json) as SkillSnapshot
  if (!data.summary || !Array.isArray(data.connections)) throw new Error("Skills connection information is incomplete. Refresh and try again.")
  return data
}

function issue(code: string, explanation: string, nextStep: string): SkillIssue { return { code, explanation, nextStep } }
function nodeIssue(node: Environment, side: "source" | "peer"): SkillIssue | undefined {
  if (node.kind === "cloud" && node.status !== "running") return {
    code: `${side.toUpperCase()}_${node.status === "error" ? "ERROR" : "STOPPED"}`, side, nodeId: node.id,
    explanation: "The cloud SSH connection is unavailable. This does not mean the remote server is powered off.",
    nextStep: "Use Connect in Yougori. Check SSH reachability, the pinned server key, identity file and Python 3 if it fails. Do not start, stop or reset the cloud server.",
  }
  const details = {
    stopped: ["STOPPED", "This node is turned off. Its services and connected shared folders are unavailable now.", "Ask the user to start this node in Yougori, wait for it to boot, then refresh Skills. Do not retry network requests while it is off."],
    paused: ["PAUSED", "This node is paused, so its programs cannot respond.", "Ask the user to resume the node, then refresh Skills. Paused is not the same as deleted or corrupted."],
    provisioning: ["STARTING", "This node is still being created or started.", "Wait for creation and guest boot to finish, then refresh Skills. Check its progress/details if it stays here; do not delete or reset it to force progress."],
    error: ["ERROR", "This node needs attention. Yougori has recorded a runtime problem; its services are not considered available.", "Ask the user to inspect this node's error in Yougori and use the recovery action offered there if appropriate. Preserve its disk and data; do not format, factory-reset or delete it."],
  } as const
  if (node.status === "running") return
  const [code, explanation, nextStep] = details[node.status]
  return { code: `${side.toUpperCase()}_${code}`, explanation, nextStep, side, nodeId: node.id }
}

export function skillIssues(source: Environment, peer: Environment | undefined, c: Connection): SkillIssue[] {
  const issues: SkillIssue[] = []
  if (!c.active) issues.push(issue("CONNECTION_DISABLED", "This connection is switched off. A saved line does not grant active access.", "If the user wants this access, ask them to reconnect the line in Yougori and refresh Skills. Do not change permissions automatically."))
  const sourceIssue = nodeIssue(source, "source"), peerIssue = peer && nodeIssue(peer, "peer")
  if (sourceIssue) issues.push(sourceIssue)
  if (peerIssue) issues.push(peerIssue)
  if (!peer) issues.push(issue("PEER_MISSING", "The other node was removed or is missing. This saved connection cannot be used.", "Ask the user to inspect the connection in the graph. Do not reuse its old IP, recreate a deleted node or redirect access to another node automatically."))
  else if (!supportsConnections(peer)) issues.push(issue("UNSUPPORTED_NODE", "This connection targets an unsupported node type.", "Connections support containers, MicroVMs and VMs. Ask the user to correct this saved connection."))
  if (c.active && source.status === "running" && peer?.status === "running") {
    if (c.enforcementStatus === "error") issues.push(issue("CONNECTION_ERROR", "Yougori could not apply the connection policy. Running nodes alone do not mean the link works.", "Read the connection error in the graph/details. It may involve the guest agent, network adapter or shared-folder setup. Ask the user to resolve it and retry the connection; do not disable firewalls or widen access."))
    else if (c.enforcementStatus === "pending") issues.push(issue("CONNECTION_PENDING", "Yougori has not finished applying this connection.", "Wait briefly and refresh Skills. If it remains pending with both nodes running, inspect the connection in Yougori instead of retrying indefinitely."))
    else if (!c.enforcementStatus) issues.push(issue("CONNECTION_UNVERIFIED", "Yougori has not confirmed that this saved connection is applied.", "Refresh Skills after Yougori checks the connection. Do not treat missing enforcement status as permission to connect."))
  }
  if (!c.permissions.length) issues.push(issue("NO_PERMISSIONS", "This connection grants no access permissions.", "Ask the user which specific access is needed; do not enable all permissions."))
  if (c.permissions.includes("ports") && !c.permissions.includes("network") && (!c.ports.length || c.ports.some(p => !/^\d+$/.test(p) || Number(p) < 1 || Number(p) > 65535))) issues.push(issue("INVALID_PORTS", "Port access has no valid TCP port list.", "Ask the user to configure the required TCP ports between 1 and 65535. Do not guess or scan ports."))
  if (c.permissions.includes("secrets") && !(source.kind === "container" && peer?.kind === "container" && (source.provider ?? "yougoriOci") === (peer.provider ?? "yougoriOci"))) issues.push(issue("UNSUPPORTED_SECRETS", "Secret-directory sharing requires two containers on the same engine.", "Ask the user to correct this connection. Do not substitute a host secret folder or copy credentials."))
  return issues
}

// Test/browser adapter uses the same instruction document and never invents live IPs.
export function previewConnectionSkill(state: PlatformState, environmentId: string, shares: HostShare[]) {
  const source = state.environments.find(e => e.id === environmentId)
  if (!source) throw new Error("Environment not found")
  if (!supportsConnections(source)) throw new Error("Skills supports containers, MicroVMs and VMs.")
  const links = nodeConnections(environmentId, state.connections)
  const connections = links.sort((a, b) => a.id.localeCompare(b.id)).map(c => {
    const peerId = c.sourceId === environmentId ? c.targetId : c.sourceId
    const peer = state.environments.find(e => e.id === peerId), issues = skillIssues(source, peer, c)
    const usable = !issues.length, outbound = c.sourceId === environmentId || c.direction === "bidirectional"
    const network = c.permissions.includes("network"), ports = c.permissions.includes("ports") && c.ports.length > 0
    const files = c.permissions.some(p => ["files", "volumes", "data"].includes(p))
    const containerPair = source.kind === "container" && peer?.kind === "container" && (source.provider ?? "yougoriOci") === (peer.provider ?? "yougoriOci")
    const mounted = source.kind === "container" || (source.kind === "microVm" && source.runtime === "builtin:alpine")
    const limitations = [
      ...(!outbound ? ["Incoming one-way connection: this node cannot initiate network requests; any shared folder is read-only from this side."] : []),
      ...(!network && !ports ? ["No network access granted on this link. File permissions do not open service ports."] : []),
      ...(!network && ports ? ["Only the listed TCP ports are allowed. UDP and ping are not port tests."] : []),
      ...(files ? ["Only the selected guest folders and connection-owned folder are shared, not the rest of either guest filesystem, My PC folders, credentials or live database storage."] : ["No Files, Volumes or Data permission: no shared connection folder is exposed."]),
    ]
    return {
      connectionId: c.id, peerId, peerName: peer?.name ?? "Missing node", peerKind: peer?.kind ?? null,
      peerStatus: peer?.status ?? "missing", peerAddress: null, usableNow: usable, active: c.active,
      issues, summary: issues[0]?.explanation ?? "Both nodes are running and the connection policy is applied. Guest boot, services, credentials and file contents have not been tested.", limitations,
      permissions: c.permissions, direction: c.direction, allowedTcpPorts: c.ports, enforcementStatus: c.enforcementStatus ?? null,
      commandsAllowedByPolicy: Boolean(c.commands && outbound), canRunPeerCommandsNow: Boolean(usable && c.commands && outbound),
      commandApi: c.commands && ![source.runtime, peer?.runtime].some(value => value?.startsWith("shared://tunnel/")) ? source.kind === "cloud" ? source.consoleEndpoint ? `${source.consoleEndpoint}/exec` : null : "http://10.192.0.1:7444/exec" : null,
      selectedFolders: (c.selectedFolders ?? []).map((folder, index) => ({ ownerId: folder.environmentId, guestPath: folder.path, apiPath: `_selected/${index}`, mountPath: folder.environmentId === source.id ? folder.path : mounted ? `/yougori/shared/${c.id}-selected-${index}` : null })),
      networkAllowedByPolicy: outbound && (network || ports), canInitiateNetwork: usable && outbound && (network || ports),
      remoteControl: c.sshPort ? { method: "SSH/SFTP", port: c.sshPort, canConnect: usable && outbound && (network || (ports && c.ports.includes(String(c.sshPort)))), authentication: "Guest SSH account and verified host key required; credentials are not included", capabilities: ["shell commands", "start/stop applications", "manage processes", "read/create/edit/delete files and code"], scope: "Permissions of the authenticated guest account" } : null,
      sharedDirectory: files && mounted && peer ? `/yougori/shared/${c.id}` : null,
      sharedDirectoryWriteAllowed: files && outbound, sharedDirectoryWritable: usable && files && outbound,
      sharedFilesReadableNow: usable && files, sharedFilesWritableNow: usable && files && outbound,
      sharedFilesBrowser: files && (!containerPair || Boolean(c.selectedFolders?.length) || Boolean(c.commands)) && peer ? source.kind === "cloud" ? source.consoleEndpoint ?? null : "http://10.192.0.1:7444" : null,
      sharedFilesApi: files && (!containerPair || Boolean(c.selectedFolders?.length) || Boolean(c.commands)) && peer ? source.kind === "cloud" ? source.consoleEndpoint ? `${source.consoleEndpoint}/api` : null : "http://10.192.0.1:7444/api" : null,
    }
  })
  const folders = shares.filter(s => s.environmentId === environmentId).sort((a, b) => a.id.localeCompare(b.id)).map(s => ({
    shareId: s.id, hostPath: s.path, mountPath: s.mountPath, readOnly: s.readOnly,
    writable: !s.readOnly, usableNow: source.status === "running",
    accessMethod: s.mountPath ? "mountedDirectory" : "privateFolderBrowser", privateLinkRequired: !s.mountPath,
  }))
  const ready = connections.filter(c => c.usableNow).length
  const data = {
    cloudProxy: source.kind === "cloud" ? source.controlEndpoint ?? null : null,
    generatedAt: new Date().toISOString(), sourceId: source.id, sourceName: source.name, sourceKind: source.kind, sourceStatus: source.status,
    scope: "direct connections only; no transitive access", serviceHealth: "not probed",
    preview: "Browser preview only. Use the desktop app for real private addresses; no live runtime was probed.",
    summary: { connectedNodes: new Set(connections.map(c => c.peerId)).size, connections: connections.length, ready, blocked: connections.length - ready },
    connections, myPc: { connected: source.status === "running" && folders.length > 0, scope: "selected folders only", folders },
  }
  return template.replace("{{CONFIGURATION}}", () => JSON.stringify(data, null, 2).replaceAll("`", "\\u0060"))
}
