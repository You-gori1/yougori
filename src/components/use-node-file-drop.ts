import { useEffect, useRef, useState, type RefObject } from "react"
import { fileImportApi, type FileCopyProgress, type FileCopyResult } from "@/api/file-import-api"
import type { Environment } from "@/types/platform"

export interface NodeFileCopy {
  busy: boolean
  names?: string[]
  progress?: FileCopyProgress
  result?: FileCopyResult
  error?: string
  cancelled?: boolean
  cancelling?: boolean
  cancelError?: string
  onCancel?: () => void
}

export function fileDropIssue(environment: Environment): string | null {
  if (environment.runtime?.startsWith("shared://") || !["container", "microVm", "fullVm", "cloud"].includes(environment.kind) || environment.provider === "nativeSandbox") return "Drop files onto a local environment or a connected SSH cloud server."
  if (environment.status !== "running") return environment.kind === "cloud" ? "Connect this cloud server before copying files into it." : "Start this environment before copying files into it."
  return null
}

/** Native drop coordinates are physical pixels; DOM hit testing uses CSS pixels. */
export function fileDropNodeAt(container: HTMLElement, position: { x: number; y: number }, scale: number): string | null {
  const pixelRatio = Number.isFinite(scale) && scale > 0 ? scale : 1
  const node = document.elementFromPoint(position.x / pixelRatio, position.y / pixelRatio)?.closest<HTMLElement>("[data-environment-id]")
  if (!node || !container.contains(node) || node.dataset.tourPreview) return null
  return node.dataset.environmentId ?? null
}

export function useNodeFileDrop(container: RefObject<HTMLElement | null>, environments: Environment[]) {
  const [hovered, setHovered] = useState<string | null>(null)
  const [copies, setCopies] = useState<Record<string, NodeFileCopy>>({})
  const current = useRef(environments)
  const locks = useRef(new Set<string>())
  useEffect(() => { current.current = environments }, [environments])
  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined
    const timers = new Map<string, ReturnType<typeof setTimeout>>()
    const update = (id: string, copy: NodeFileCopy) => {
      if (!disposed) setCopies(previous => ({ ...previous, [id]: copy }))
    }
    void fileImportApi.listen(event => {
      if (disposed) return
      if (event.type === "leave") { setHovered(null); return }
      const id = container.current ? fileDropNodeAt(container.current, event.position, window.devicePixelRatio) : null
      const environment = current.current.find(item => item.id === id)
      if (event.type !== "drop") { setHovered(environment?.id ?? null); return }
      setHovered(null)
      if (!environment || !event.paths.length || locks.current.has(environment.id)) return
      const issue = fileDropIssue(environment)
      if (issue) { update(environment.id, { busy: false, error: issue }); return }
      const target = environment.id
      clearTimeout(timers.get(target))
      const names=event.paths.map(path=>path.replace(/[\\/]+$/, "").split(/[\\/]/).pop()??path)
      locks.current.add(target)
      let lastProgress: FileCopyProgress = { phase: "preparing", completedBytes: 0, totalBytes: 0 }
      let cancelling = false
      let cancelError: string | undefined
      const onCancel = () => {
        if (cancelling || !locks.current.has(target)) return
        cancelling = true
        cancelError = undefined
        update(target, { busy: true, names, progress: lastProgress, cancelling, onCancel })
        void fileImportApi.cancel(target, lastProgress.transferId)
          .then(result => {
            if (!result.cancelRequested && locks.current.has(target)) {
              cancelling = false
              cancelError = "This transfer has finished or is no longer registered. Its final result will appear here."
              update(target, { busy: true, names, progress: lastProgress, cancelling, cancelError, onCancel })
            }
          })
          .catch((error: unknown) => {
            cancelling = false
            cancelError = error instanceof Error ? error.message : String(error)
            if (locks.current.has(target)) update(target, { busy: true, names, progress: lastProgress, cancelling, cancelError, onCancel })
          })
      }
      update(target, { busy: true, names, progress: lastProgress, onCancel })
      void fileImportApi.copy(target, [...event.paths], progress => { lastProgress = progress; update(target, { busy: true, names, progress, cancelling, cancelError, onCancel }) })
        .then(result => { if(disposed)return; update(target, { busy: false, result, names }); timers.set(target,setTimeout(()=>{if(!disposed)setCopies(previous=>{const next={...previous};delete next[target];return next});timers.delete(target)},15_000)) })
        .catch((error: unknown) => {
          const message = error instanceof Error ? error.message : String(error)
          const cancelled = message.startsWith("YOUGORI_OPERATION_CANCELLED:")
          update(target, { busy: false, error: message.replace(/^YOUGORI_[A-Z_]+:\s*/, ""), cancelled, names, progress: lastProgress })
        })
        .finally(() => locks.current.delete(target))
    }).then(stop => { if (disposed) stop(); else unlisten = stop })
      .catch(() => { /* A window being closed may reject listener registration. */ })
    return () => { disposed = true; unlisten?.(); timers.forEach(clearTimeout) }
  }, [container])
  return { hovered, copies }
}
