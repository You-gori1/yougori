// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { PlatformState } from "@/types/platform"
import { ConnectionDialog, FolderBrowser } from "./connection-dialog"
import { platformApi } from "@/api/platform-api"

const { save, deleteLink, state } = vi.hoisted(() => ({ save: vi.fn().mockResolvedValue(undefined), deleteLink: vi.fn().mockResolvedValue(undefined), state: { environments: [
  { id: "env-a", name: "A", kind: "container", runtime: "alpine", status: "running" },
  { id: "env-b", name: "B", kind: "cloud", runtime: "ssh://server", status: "running" },
], connections: [
  { id: "conn-existing", sourceId: "env-a", targetId: "env-b", direction: "bidirectional", permissions: ["files"], ports: [], active: true, createdAt: "2026-01-01" },
] } as unknown as PlatformState }))
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ state, createConnection: save, deleteConnection: deleteLink }) }))
beforeEach(() => vi.stubGlobal("PointerEvent", MouseEvent))
afterEach(() => { cleanup(); save.mockClear(); deleteLink.mockClear(); vi.restoreAllMocks(); vi.unstubAllGlobals() })

it("opens an existing Files connection with Data selected for editing", async () => {
  const close = vi.fn()
  render(<ConnectionDialog open onOpenChange={close} initialSourceId="env-a" initialTargetId="env-b" />)
  expect(await screen.findByRole("heading", { name: "Edit connection" })).toBeInTheDocument()
  expect(screen.queryByRole("checkbox", { name: "Files" })).not.toBeInTheDocument()
  expect(screen.getByRole("checkbox", { name: "Share data" })).toBeChecked()
  expect(screen.queryByRole("checkbox", { name: "Shared volumes" })).not.toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Save connection" })).toBeInTheDocument()
  expect(save).not.toHaveBeenCalled()
  expect(close).not.toHaveBeenCalled()
})

it("removes an existing connection when its final capability is cleared", async () => {
  const close = vi.fn()
  render(<ConnectionDialog open onOpenChange={close} initialSourceId="env-a" initialTargetId="env-b" />)
  fireEvent.click(await screen.findByRole("checkbox", { name: "Share data" }))
  expect(screen.getByRole("button", { name: "Remove connection" })).toBeInTheDocument()
  fireEvent.click(screen.getByRole("button", { name: "Remove connection" }))
  await waitFor(() => expect(deleteLink).toHaveBeenCalledWith("conn-existing"))
  expect(save).not.toHaveBeenCalled()
  expect(screen.queryByText("Select at least one permission.")).not.toBeInTheDocument()
  expect(close).toHaveBeenCalledWith(false)
})

it("browses a running guest and selects only its chosen folder", async () => {
  const select = vi.fn()
  const folders = vi.spyOn(platformApi, "listEnvironmentFolders").mockImplementation(async (_id, path) => ({ path: path || "/workspace", entries: !path || path === "/workspace" ? [{ name: "project", directory: true }, { name: "readme.md", directory: false }] : [{ name: "package.json", directory: false }] }))
  render(<FolderBrowser environment={state.environments[0]!} selected={[]} onSelect={select} disabled={false} />)
  expect(folders).toHaveBeenCalledWith("env-a", "")
  expect(screen.getByRole("button", { name: "Up" })).toBeDisabled()
  expect(await screen.findByText("readme.md")).toBeInTheDocument()
  expect(screen.queryByRole("button", { name: "readme.md" })).not.toBeInTheDocument()
  fireEvent.click(await screen.findByRole("button", { name: "project" }))
  expect(screen.getByRole("button", { name: "Up" })).toBeEnabled()
  expect(await screen.findByText("package.json")).toBeInTheDocument()
  await waitFor(() => expect(screen.getByRole("button", { name: "Share folder" })).toBeEnabled())
  fireEvent.click(screen.getByRole("button", { name: "Share folder" }))
  expect(select).toHaveBeenCalledWith("/workspace/project")
})

it("requires a specific folder when the guest starts at filesystem root", async () => {
  const select = vi.fn()
  vi.spyOn(platformApi, "listEnvironmentFolders").mockImplementation(async (_id, path) => ({ path: path || "/", entries: [{ name: "work", directory: true }, { name: "readme.txt", directory: false }] }))
  render(<FolderBrowser environment={state.environments[0]!} selected={[]} onSelect={select} disabled={false} />)
  const share = await screen.findByRole("button", { name: "Share folder" })
  expect(share).toBeDisabled()
  expect(screen.getByText("Open a project or data folder to share it. The entire filesystem cannot be shared.")).toBeInTheDocument()
  fireEvent.click(await screen.findByRole("button", { name: "work" }))
  await waitFor(() => expect(share).toBeEnabled())
  fireEvent.click(share)
  expect(select).toHaveBeenCalledWith("/work")
})

it("does not save a legacy root share until the broad selection is removed", async () => {
  const connection = state.connections[0]!
  connection.selectedFolders = [{ environmentId: "env-a", path: "/" }]
  try {
    render(<ConnectionDialog open onOpenChange={vi.fn()} initialSourceId="env-a" initialTargetId="env-b" />)
    expect(screen.getByText(/This connection shares the entire filesystem/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Save connection" }))
    expect(screen.getByText("Remove the / selection and choose a specific project or data folder.")).toBeInTheDocument()
    expect(save).not.toHaveBeenCalled()
  } finally {
    connection.selectedFolders = []
  }
})

it("runs a command only through an existing command-enabled connection", async () => {
  const connection = state.connections[0]!
  connection.commands = true
  connection.enforcementStatus = "enforced"
  const execute = vi.spyOn(platformApi, "executeConnectedCommand").mockResolvedValue({ exitCode: 0, stdout: "file.txt\n", stderr: "" })
  try {
    render(<ConnectionDialog open onOpenChange={vi.fn()} initialSourceId="env-a" initialTargetId="env-b" />)
    fireEvent.change(screen.getByRole("textbox", { name: "Peer command" }), { target: { value: "ls" } })
    fireEvent.click(screen.getByRole("button", { name: "Run" }))
    expect(execute).toHaveBeenCalledWith("conn-existing", "env-a", "ls")
  } finally {
    connection.commands = false
    connection.enforcementStatus = undefined
  }
})
