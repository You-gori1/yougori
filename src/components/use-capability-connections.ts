import { useCallback, useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type RefObject } from "react"
import { roundedConnectionPath, routeConnections, type ConnectionRoute } from "@/lib/graph-connection-paths"
import { allCapabilities, capabilityEnabled, capabilityIssue, endpointPair, publicationDestination, sameEndpoint, savedSetupForPublication, serviceId, type CapabilityEndpoint, type CapabilityKind, type GraphEnvironment } from "@/components/graph-capabilities"
import type { PublicationKind } from "@/api/workspace-api"
import { readPublicAccessPresets } from "@/lib/public-access-presets"

interface Point { x: number; y: number; side?: "top" | "bottom" | "left" | "right" }
interface Line { id: string; path: string; environmentId: string; inactive: boolean }
interface Gesture { origin: CapabilityEndpoint; start: Point; pointerId: number; element: HTMLElement; dragged: boolean }

/** Pointer capture works across the fixed dock and the zoomable canvas, including touch. */
export function useCapabilityConnections(
  containerRef: RefObject<HTMLDivElement | null>,
  environments: GraphEnvironment[],
  save: (environmentId: string, capability: CapabilityKind, enabled: boolean) => Promise<void>,
  publish: (environmentId: string, port: number, kind: PublicationKind, presetId?: string) => Promise<void>,
) {
  const [active, setActive] = useState<CapabilityEndpoint | null>(null)
  const [hovered, setHovered] = useState<CapabilityEndpoint | null>(null)
  const [lines, setLines] = useState<Line[]>([])
  const [preview, setPreview] = useState("")
  const [feedback, setFeedback] = useState("")
  const [pending, setPending] = useState<Set<string>>(new Set())
  const activeRef = useRef<CapabilityEndpoint | null>(null)
  const gestureRef = useRef<Gesture | null>(null)
  const cursorRef = useRef<Point | null>(null)
  const pendingRef = useRef(new Set<string>())
  const suppressClickRef = useRef(false)
  const frameRef = useRef<number | null>(null)

  const select = useCallback((endpoint: CapabilityEndpoint | null) => {
    activeRef.current = endpoint
    setActive(endpoint)
    setHovered(null)
    setPreview("")
  }, [])

  const cancel = useCallback(() => {
    const gesture = gestureRef.current
    gestureRef.current = null
    if (gesture?.element.hasPointerCapture(gesture.pointerId)) gesture.element.releasePointerCapture(gesture.pointerId)
    cursorRef.current = null
    select(null)
  }, [select])

  // Choose the nearest visible card/port. The forgiving margin also catches drops just outside a dot.
  const targetAt = useCallback((point: Point, origin: CapabilityEndpoint): CapabilityEndpoint | null => {
    const container = containerRef.current
    if (!container) return null
    const canvas = container.querySelector<HTMLElement>("[data-environment-canvas]")?.getBoundingClientRect()
    const inCanvas = origin.kind === "capability" || origin.kind === "publication" || origin.kind === "preset"
    if (inCanvas && (!canvas || point.y < canvas.top || point.y > canvas.bottom || point.x < canvas.left || point.x > canvas.right)) return null
    const selector = origin.kind === "capability" ? "[data-environment-id]" : origin.kind === "environment" ? "[data-capability-card]" : origin.kind === "publication" || origin.kind === "preset" ? "[data-service-card]" : "[data-publication-card], [data-preset-card]"
    const endpointFor = (element: HTMLElement): CapabilityEndpoint => {
      if (origin.kind === "capability") return { kind: "environment", id: element.dataset.environmentId! }
      if (origin.kind === "environment") return { kind: "capability", id: element.dataset.capabilityCard as CapabilityKind }
      if (origin.kind === "publication" || origin.kind === "preset") return { kind: "service", id: element.dataset.serviceCard! }
      return element.dataset.presetCard ? { kind: "preset", id: element.dataset.presetCard } : { kind: "publication", id: element.dataset.publicationCard as PublicationKind }
    }
    // Respect the visible stacking order when the user overlaps two nodes.
    const hit = document.elementFromPoint(point.x, point.y)?.closest<HTMLElement>(selector)
    if (hit && container.contains(hit)) return endpointFor(hit)
    let closest: { endpoint: CapabilityEndpoint; distance: number } | null = null
    for (const element of container.querySelectorAll<HTMLElement>(selector)) {
      const rect = element.getBoundingClientRect()
      if (inCanvas && canvas && (rect.bottom < canvas.top || rect.top > canvas.bottom || rect.right < canvas.left || rect.left > canvas.right)) continue
      const dx = Math.max(rect.left - point.x, 0, point.x - rect.right)
      const dy = Math.max(rect.top - point.y, 0, point.y - rect.bottom)
      const distance = Math.hypot(dx, dy)
      if (distance > 24 || (closest && closest.distance <= distance)) continue
      closest = { endpoint: endpointFor(element), distance }
    }
    return closest?.endpoint ?? null
  }, [containerRef])

  const updateGeometry = useCallback(() => {
    const container = containerRef.current
    const svg = container?.querySelector<SVGSVGElement>("[data-capability-lines]")
    const canvas = container?.querySelector<HTMLElement>("[data-environment-canvas]")?.getBoundingClientRect()
    if (!container || !svg || !canvas) return
    const bounds = svg.getBoundingClientRect()
    const points = new Map<string, Point>()
    for (const element of container.querySelectorAll<HTMLElement>("[data-environment-connection-point], [data-capability-connection-point], [data-service-connection-point], [data-publication-connection-point], [data-preset-connection-point]")) {
      const rect = element.getBoundingClientRect()
      const x = rect.left + rect.width / 2
      const y = rect.top + rect.height / 2
      const environmentId = element.dataset.environmentConnectionPoint
      const service = element.dataset.serviceConnectionPoint
      const destinations = element.closest(".workspace-connection-scroll")?.getBoundingClientRect()
      // Off-screen nodes must not leave wires hanging over the dock or outside the canvas.
      if ((environmentId || service) && (x < canvas.left || x > canvas.right || y < canvas.top || y > canvas.bottom - 2)) continue
      if ((element.dataset.capabilityConnectionPoint || element.dataset.publicationConnectionPoint || element.dataset.presetConnectionPoint) && destinations && (x < destinations.left || x > destinations.right)) continue
      const key = environmentId ? `environment:${environmentId}` : service ? `service:${service}` : element.dataset.presetConnectionPoint ? `preset:${element.dataset.presetConnectionPoint}` : element.dataset.publicationConnectionPoint ? `publication:${element.dataset.publicationConnectionPoint}` : `capability:${element.dataset.capabilityConnectionPoint}`
      points.set(key, { x: x - bounds.left, y: y - bounds.top, side: element.dataset.connectionSide as Point["side"] ?? (environmentId ? "bottom" : "top") })
    }
    const presets = readPublicAccessPresets()
    const routes: ConnectionRoute[] = []
    const owners = new Map<string, string>()
    for (const environment of environments) {
      const target = points.get(`environment:${environment.id}`)
      for (const { capability } of allCapabilities) {
        owners.set(`${capability}:${environment.id}`, environment.id)
        const source = points.get(`capability:${capability}`)
        if (source && target && capabilityEnabled(environment, capability)) routes.push({ id: `${capability}:${environment.id}`, sourceKey: `capability:${capability}`, targetKey: `environment:${environment.id}`, source, target })
      }
      const connectedDestinations = new Set<string>()
      for (const publication of environment.workspace?.publications ?? []) {
        const source = points.get(`service:${serviceId(environment.id, publication.port)}`)
        const kind = publicationDestination(publication.kind)
        const preset = savedSetupForPublication(publication, presets)
        const destinationKey = preset ? `preset:${preset.id}` : `publication:${kind}`
        const destination = points.get(destinationKey)
        const id = `pub-${serviceId(environment.id, publication.port)}:${destinationKey}`
        owners.set(id, environment.id)
        if (source && destination && !connectedDestinations.has(id)) {
          routes.push({ id, sourceKey: `service:${serviceId(environment.id, publication.port)}`, targetKey: destinationKey, source, target: destination })
          connectedDestinations.add(id)
        }
      }
    }
    const next = routeConnections(routes).map(line => {
      const environmentId = owners.get(line.id)!
      return { ...line, environmentId, inactive: environments.find(environment => environment.id === environmentId)?.status !== "running" }
    })
    setLines(current => current.length === next.length && current.every((line, i) => line.id === next[i]?.id && line.path === next[i]?.path && line.inactive === next[i]?.inactive) ? current : next)

    const origin = activeRef.current
    const cursor = cursorRef.current
    const target = origin && cursor ? targetAt(cursor, origin) : null
    setHovered(current => sameEndpoint(current, target) ? current : target)
    const start = origin && points.get(`${origin.kind}:${origin.id}`)
    const pair = origin && target ? endpointPair(origin, target) : null
    const environment = pair && environments.find(item => item.id === pair.environmentId)
    const presetMatches = pair?.type !== "publication" || !("presetId" in pair) || presets.some(item => item.id === pair.presetId && item.port === pair.port)
    const canSnap = pair && environment && presetMatches && (pair.type === "publication" ? environment.status === "running" : !capabilityIssue(environment, pair.capability)) && !pendingRef.current.has(environment.id)
    const end = canSnap && target ? points.get(`${target.kind}:${target.id}`) : null
    const pointer = cursor ? { x: cursor.x - bounds.left, y: cursor.y - bounds.top } : null
    setPreview(start && pointer ? origin?.kind === "capability" ? roundedConnectionPath(start, end ?? pointer) : roundedConnectionPath(end ?? pointer, start) : "")
  }, [containerRef, environments, targetAt])

  const scheduleGeometry = useCallback(() => {
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current)
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null
      updateGeometry()
    })
  }, [updateGeometry])

  // Observe card size as well as the wrapper: attaching a label changes the node's height.
  useLayoutEffect(() => {
    const container = containerRef.current
    if (!container) return
    const observer = new ResizeObserver(scheduleGeometry)
    observer.observe(container)
    const observeCards = () => container.querySelectorAll("[data-environment-id], [data-capability-card], [data-publication-card], [data-preset-card]").forEach(element => observer.observe(element))
    observeCards()
    // Controlled ReactFlow nodes can mount one commit after the environment data arrives.
    const observeFrame = requestAnimationFrame(observeCards)
    scheduleGeometry()
    window.addEventListener("scroll", scheduleGeometry, true)
    window.addEventListener("yougori-public-presets-changed", scheduleGeometry)
    return () => {
      observer.disconnect()
      cancelAnimationFrame(observeFrame)
      window.removeEventListener("scroll", scheduleGeometry, true)
      window.removeEventListener("yougori-public-presets-changed", scheduleGeometry)
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current)
    }
  }, [containerRef, scheduleGeometry])

  useEffect(() => {
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") { cancel(); setFeedback("") } }
    const outside = (event: PointerEvent) => {
      suppressClickRef.current = false
      if (!containerRef.current?.contains(event.target as Node)) cancel()
    }
    document.addEventListener("keydown", escape)
    document.addEventListener("pointerdown", outside)
    window.addEventListener("blur", cancel)
    return () => {
      document.removeEventListener("keydown", escape)
      document.removeEventListener("pointerdown", outside)
      window.removeEventListener("blur", cancel)
    }
  }, [cancel, containerRef])

  const change = useCallback(async (environmentId: string, capability: CapabilityKind, enabled: boolean) => {
    const environment = environments.find(item => item.id === environmentId)
    if (!environment || pendingRef.current.has(environmentId)) return
    if (capability !== "pc" && capabilityEnabled(environment, capability) === enabled) { cancel(); return }
    const issue = capabilityIssue(environment, capability)
    if (issue) { setFeedback(issue); return }
    cancel()
    setFeedback("")
    pendingRef.current.add(environmentId)
    setPending(new Set(pendingRef.current))
    try { await save(environmentId, capability, enabled) }
    catch (error) { setFeedback(error instanceof Error ? error.message : String(error)) }
    finally {
      pendingRef.current.delete(environmentId)
      setPending(new Set(pendingRef.current))
    }
  }, [cancel, environments, save])

  const complete = useCallback((origin: CapabilityEndpoint, target: CapabilityEndpoint) => {
    const pair = endpointPair(origin, target)
    if (!pair) return
    if (pair.type === "capability") { void change(pair.environmentId, pair.capability, true); return }
    const environment = environments.find(item => item.id === pair.environmentId)
    if (!environment || pendingRef.current.has(environment.id)) return
    cancel()
    const presetId = "presetId" in pair ? pair.presetId : undefined
    if (presetId && !readPublicAccessPresets().some(item => item.id === presetId && item.port === pair.port)) { setFeedback("This setup is for a different app port or is no longer saved."); return }
    if (environment.status !== "running") { setFeedback("Start the environment before publishing a service."); return }
    if (environment.kind === "cloud") { setFeedback("Cloud nodes cannot connect to Local network or Public access."); return }
    pendingRef.current.add(environment.id); setPending(new Set(pendingRef.current)); setFeedback("")
    void publish(environment.id, pair.port, pair.publication, presetId).catch(error => setFeedback(error instanceof Error ? error.message : String(error))).finally(() => {
      pendingRef.current.delete(environment.id); setPending(new Set(pendingRef.current))
    })
  }, [cancel, change, environments, publish])

  const clickEndpoint = useCallback((endpoint: CapabilityEndpoint) => {
    if (suppressClickRef.current) { suppressClickRef.current = false; return }
    setFeedback("")
    const current = activeRef.current
    if (current && endpointPair(current, endpoint)) { complete(current, endpoint); return }
    cursorRef.current = null
    select(sameEndpoint(current, endpoint) ? null : endpoint)
  }, [complete, select])

  const consumeClick = useCallback(() => {
    const suppressed = suppressClickRef.current
    suppressClickRef.current = false
    return suppressed
  }, [])

  const pointerDown = useCallback((event: ReactPointerEvent<HTMLElement>, origin: CapabilityEndpoint) => {
    if (event.button !== 0 || !event.isPrimary) return
    event.stopPropagation()
    suppressClickRef.current = false
    gestureRef.current = { origin, start: { x: event.clientX, y: event.clientY }, pointerId: event.pointerId, element: event.currentTarget, dragged: false }
    event.currentTarget.setPointerCapture(event.pointerId)
  }, [])

  const pointerMove = useCallback((event: ReactPointerEvent) => {
    const gesture = gestureRef.current
    if (gesture && gesture.pointerId !== event.pointerId) return
    const point = { x: event.clientX, y: event.clientY }
    if (gesture && !gesture.dragged && Math.hypot(point.x - gesture.start.x, point.y - gesture.start.y) >= 4) {
      gesture.dragged = true
      // Keep the banner in place until the drop finishes, so clearing an old
      // error cannot move the graph underneath an active pointer gesture.
      select(gesture.origin)
    }
    if (!activeRef.current) return
    cursorRef.current = point
    scheduleGeometry()
  }, [scheduleGeometry, select])

  const pointerUp = useCallback((event: ReactPointerEvent) => {
    const gesture = gestureRef.current
    if (!gesture || gesture.pointerId !== event.pointerId) return
    gestureRef.current = null
    if (gesture.element.hasPointerCapture(event.pointerId)) gesture.element.releasePointerCapture(event.pointerId)
    if (!gesture.dragged) return
    suppressClickRef.current = true
    const target = targetAt({ x: event.clientX, y: event.clientY }, gesture.origin)
    cancel()
    if (target) complete(gesture.origin, target)
  }, [cancel, complete, targetAt])

  return { active, hovered, lines, preview, feedback, pending, dismissFeedback: () => setFeedback(""), cancel, change, clickEndpoint, consumeClick, pointerDown, pointerMove, pointerUp, scheduleGeometry }
}
