import { getSmoothStepPath, Position } from "@xyflow/react"

export interface ConnectionPoint { x: number; y: number; side?: "top" | "bottom" | "left" | "right" }
export interface ConnectionRoute { id: string; sourceKey: string; targetKey: string; source: ConnectionPoint; target: ConnectionPoint }
const positions = { top: Position.Top, bottom: Position.Bottom, left: Position.Left, right: Position.Right }
const directions = { top: { x: 0, y: -1 }, bottom: { x: 0, y: 1 }, left: { x: -1, y: 0 }, right: { x: 1, y: 0 } }

function spread(index: number, count: number, spacing = 14, maximum = 72) {
  return (index - (count - 1) / 2) * Math.min(spacing, maximum / Math.max(1, count - 1))
}

export function roundedConnectionPath(source: ConnectionPoint, target: ConnectionPoint, sourceFan = 0, targetFan = 0, lane = 0) {
  const sourceSide = source.side ?? "top", targetSide = target.side ?? "bottom"
  const a = directions[sourceSide], b = directions[targetSide]
  // Separate wires immediately at shared sockets, while retaining the exact
  // socket centers as endpoints. Only the short fan-out uses a gentle curve.
  const start = { x: source.x + a.x * 24 + (a.y ? sourceFan : 0), y: source.y + a.y * 24 + (a.x ? sourceFan : 0) }
  const end = { x: target.x + b.x * 24 + (b.y ? targetFan : 0), y: target.y + b.y * 24 + (b.x ? targetFan : 0) }
  const [middle] = getSmoothStepPath({
    sourceX: start.x, sourceY: start.y, targetX: end.x, targetY: end.y,
    sourcePosition: positions[sourceSide], targetPosition: positions[targetSide],
    centerX: (start.x + end.x) / 2 + lane,
    centerY: (start.y + end.y) / 2 + lane,
    offset: 20, borderRadius: 14,
  })
  return `M ${source.x} ${source.y} C ${source.x + a.x * 12} ${source.y + a.y * 12}, ${start.x - a.x * 12} ${start.y - a.y * 12}, ${start.x} ${start.y} ${middle.replace(/^M[^LQCHV]*/, "")} C ${end.x - b.x * 12} ${end.y - b.y * 12}, ${target.x + b.x * 12} ${target.y + b.y * 12}, ${target.x} ${target.y}`
}

export function routeConnections(routes: ConnectionRoute[]) {
  const ordered = [...routes].sort((a, b) => a.id.localeCompare(b.id))
  const groups = new Map<string, ConnectionRoute[]>()
  for (const route of ordered) {
    for (const key of [`source:${route.sourceKey}`, `target:${route.targetKey}`]) {
      const group = groups.get(key) ?? []
      group.push(route)
      groups.set(key, group)
    }
  }
  const fan = (route: ConnectionRoute, source: boolean) => {
    const group = groups.get(`${source ? "source" : "target"}:${source ? route.sourceKey : route.targetKey}`)!
    const socket = source ? route.source : route.target
    const horizontal = socket.side === "left" || socket.side === "right"
    const sorted = [...group].sort((a, b) => {
      const p = source ? a.target : a.source, q = source ? b.target : b.source
      return (horizontal ? p.y - q.y : p.x - q.x) || a.id.localeCompare(b.id)
    })
    return spread(sorted.indexOf(route), sorted.length)
  }
  return ordered.map((route, index) => ({ id: route.id, path: roundedConnectionPath(route.source, route.target,
    fan(route, true), fan(route, false), spread(index, ordered.length, 16, 120)) }))
}
