// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { act, cleanup, render, renderHook, screen, waitFor } from "@testing-library/react"
import { createRef } from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { PhysicalPosition } from "@tauri-apps/api/dpi"
import type { DragDropEvent } from "@tauri-apps/api/webview"
import { fileImportApi, type FileCopyProgress, type FileCopyResult } from "@/api/file-import-api"
import { fileDropIssue, useNodeFileDrop } from "@/components/use-node-file-drop"
import { NodeFileCopyStatus } from "@/components/node-file-copy-status"
import type { Environment } from "@/types/platform"

vi.mock("@/api/file-import-api", () => ({ fileImportApi: { listen: vi.fn(), copy: vi.fn() } }))
const environment = { id: "node-one", name: "Project", kind: "container", provider: "yougoriOci", status: "running" } as Environment
const copied: FileCopyResult = { destination: "/yougori-import-example", files: 2, bytes: 42, skippedLinks: 0, delivery: "directory" }
let callback: (event: DragDropEvent) => void
let hit: HTMLElement | null
let container: ReturnType<typeof createRef<HTMLDivElement>>
let node: HTMLElement
const stop = vi.fn()

beforeEach(() => {
  vi.clearAllMocks()
  container = createRef<HTMLDivElement>()
  container.current = document.createElement("div")
  node = document.createElement("article")
  node.dataset.environmentId = environment.id
  container.current.append(node)
  document.body.append(container.current)
  hit = node
  Object.defineProperty(document, "elementFromPoint", { configurable: true, value: vi.fn(() => hit) })
  Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: 2 })
  vi.mocked(fileImportApi.listen).mockImplementation(async handler => { callback = handler; return stop })
  vi.mocked(fileImportApi.copy).mockResolvedValue(copied)
})
afterEach(() => { cleanup(); container.current?.remove(); vi.useRealTimers() })
const send = (type: "enter" | "drop", paths = ["C:\\Projects\\my project"]) => act(() => callback({ type, paths, position: new PhysicalPosition(600, 400) }))

describe("native files dropped onto graph nodes", () => {
  it("removes imported names and destinations exactly 15 seconds after success", async()=>{
    vi.useFakeTimers()
    const {result}=renderHook(()=>useNodeFileDrop(container,[environment]))
    await act(async()=>send("drop",["C:\\Projects\\my project","C:\\notes.txt"]))
    expect(result.current.copies[environment.id]?.names).toEqual(["my project","notes.txt"])
    act(()=>vi.advanceTimersByTime(14_999))
    expect(result.current.copies[environment.id]?.result).toEqual(copied)
    act(()=>vi.advanceTimersByTime(1))
    expect(result.current.copies[environment.id]).toBeUndefined()
  })
  it("uses physical coordinates at high DPI and copies only to the hit node", async () => {
    const { result } = renderHook(() => useNodeFileDrop(container, [environment]))
    send("enter")
    expect(result.current.hovered).toBe(environment.id)
    expect(document.elementFromPoint).toHaveBeenLastCalledWith(300, 200)
    send("drop", ["C:\\Projects\\my project", "C:\\notes.txt"])
    await waitFor(() => expect(result.current.copies[environment.id]?.result).toEqual(copied))
    expect(fileImportApi.copy).toHaveBeenCalledExactlyOnceWith(environment.id, ["C:\\Projects\\my project", "C:\\notes.txt"], expect.any(Function))
    expect(result.current.hovered).toBeNull()
  })

  it("ignores drops outside nodes, on tutorial previews, or behind a dialog", () => {
    renderHook(() => useNodeFileDrop(container, [environment]))
    hit = document.body
    send("drop")
    hit = node
    node.dataset.tourPreview = "true"
    send("drop")
    hit = document.createElement("article")
    hit.dataset.environmentId = environment.id
    send("drop")
    expect(fileImportApi.copy).not.toHaveBeenCalled()
  })

  it.each(["stopped", "paused", "provisioning", "error"] as const)("does not copy into a %s environment or start it implicitly", status => {
    const { result } = renderHook(() => useNodeFileDrop(container, [{ ...environment, status }]))
    send("drop")
    expect(result.current.copies[environment.id]?.error).toMatch(/Start this environment/)
    expect(fileImportApi.copy).not.toHaveBeenCalled()
  })

  it("supports local and SSH cloud environments, but rejects shared nodes and computer branches", () => {
    for (const kind of ["container", "microVm", "fullVm", "cloud"] as const) expect(fileDropIssue({ ...environment, kind, runtime: "ssh-cloud" })).toBeNull()
    expect(fileDropIssue({ ...environment, provider: "yougoriCuda" })).toBeNull()
    expect(fileDropIssue({ ...environment, kind: "cloud", runtime: "shared://tunnel/example" })).not.toBeNull()
    expect(fileDropIssue({ ...environment, kind: "cloud", runtime: "ssh-cloud", status: "stopped" })).toMatch(/Connect this cloud server/)
    expect(fileDropIssue({ ...environment, kind: "computerBranch" })).not.toBeNull()
  })

  it("shows progress without a dialog and blocks duplicate drops until the copy finishes", async () => {
    let resolve!: (value: FileCopyResult) => void
    let report!: (progress: FileCopyProgress) => void
    vi.mocked(fileImportApi.copy).mockImplementation((_id, _paths, progress) => { report = progress; return new Promise(done => { resolve = done }) })
    function Fixture() { const drop = useNodeFileDrop(container, [environment]); return <NodeFileCopyStatus copy={drop.copies[environment.id]} /> }
    render(<Fixture />)
    send("drop")
    send("drop")
    expect(fileImportApi.copy).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    expect(screen.getByRole("status")).toHaveTextContent("Preparing copy")
    act(() => report({ phase: "scanning", completedBytes: 0, totalBytes: 0, scannedEntries: 120000 }))
    expect(screen.getByRole("status")).toHaveTextContent("Scanning folder")
    expect(screen.getByRole("status")).toHaveTextContent("120,000 items found")
    act(() => report({ phase: "copying", completedBytes: 25, totalBytes: 100 }))
    expect(screen.getByRole("status")).toHaveTextContent("25%")
    expect(screen.getByRole("status")).toHaveTextContent("Originals stay on your computer")
    await act(async () => resolve({ ...copied, skippedLinks: 1 }))
    expect(screen.getByRole("status")).toHaveTextContent(copied.destination)
    expect(screen.getByRole("status")).toHaveTextContent("Skipped 1 symbolic link")
  })

  it("reports backend failures and permits retry without reporting a false success", async () => {
    vi.mocked(fileImportApi.copy).mockRejectedValueOnce(new Error("Not enough disk space"))
    const { result } = renderHook(() => useNodeFileDrop(container, [environment]))
    send("drop")
    await waitFor(() => expect(result.current.copies[environment.id]?.error).toBe("Not enough disk space"))
    expect(result.current.copies[environment.id]?.result).toBeUndefined()
    send("drop")
    await waitFor(() => expect(result.current.copies[environment.id]?.result).toEqual(copied))
  })

  it("unsubscribes even when native listener registration finishes after unmount", async () => {
    let registered!: (stop: () => void) => void
    vi.mocked(fileImportApi.listen).mockImplementation(() => new Promise(resolve => { registered = resolve }))
    const { unmount } = renderHook(() => useNodeFileDrop(container, [environment]))
    unmount()
    await act(async () => registered(stop))
    expect(stop).toHaveBeenCalledOnce()
  })
})
