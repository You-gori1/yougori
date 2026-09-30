// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { RemoteWorkspace, RemoteFiles } from "./remote-workspace"
import type { Environment } from "@/types/platform"
const { inspect, files, reconnect, refreshPlatform, power, openWindow, environmentActions } = vi.hoisted(() => ({ inspect: vi.fn(), files: vi.fn(), reconnect: vi.fn(), refreshPlatform: vi.fn(), power: vi.fn(), openWindow: vi.fn(), environmentActions: {} as Record<string, string> }))
vi.mock("@/api/remote-access-api", () => ({ remoteAccessApi: { inspect, files, reconnect } }))
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ setEnvironmentStatus: power, openEnvironmentWindow: openWindow, environmentActions, refreshPlatform }) }))
vi.mock("./sharing-panel", () => ({ SharingPanel: () => <button>Connect again</button> }))
vi.mock("./guest-terminal", () => ({ GuestTerminal: ({ installer }: { installer?: string }) => <div>{installer ? `Installer terminal: ${installer}` : "Remote terminal"}</div> }))
vi.mock("./guest-logs", () => ({ GuestLogs: () => <div>Remote logs</div> }))
vi.mock("./connection-skills-dialog", () => ({ ConnectionSkillsDialog: () => <button>Skills</button> }))
vi.mock("./guest-app-launcher", () => ({ GuestAppLauncher: () => <button>Apps</button> }))
const environment = { id: "remote", name: "Project (remote)", status: "running" } as Environment
afterEach(() => { cleanup(); vi.resetAllMocks(); Object.keys(environmentActions).forEach(id => delete environmentActions[id]) })
it("disables shared power controls while an action for that environment is pending", async () => {
  inspect.mockResolvedValue({ permission: "control", files: false, commands: true, power: true })
  environmentActions.remote = "stopping"
  render(<RemoteWorkspace environment={environment} onClose={() => {}} />)
  const stop = await screen.findByRole("button", { name: "Stop" })
  expect(stop).toBeDisabled()
  expect(screen.getByRole("button", { name: "Restart" })).toBeDisabled()
  fireEvent.click(stop)
  expect(power).not.toHaveBeenCalled()
})
it("shows a read-only workspace without command or power controls", async () => {
  inspect.mockResolvedValue({ permission: "view", files: true, commands: false, power: false })
  files.mockResolvedValue({ entries: [] })
  render(<RemoteWorkspace environment={environment} onClose={() => {}} />)
  expect(await screen.findByText("Read only")).toBeInTheDocument()
  expect(screen.queryByRole("tab", { name: "Terminal" })).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: "Stop" })).not.toBeInTheDocument()
  expect(screen.queryByLabelText("Upload files")).not.toBeInTheDocument()
  expect(screen.getByLabelText("Shared file contents")).toHaveAttribute("readonly")
  expect(screen.getByRole("button", { name: "Install tools" })).toBeDisabled()
  expect(screen.getByRole("button", { name: "Apps" })).toBeDisabled()
})
it("file editing does not imply terminal permission", async () => {
  inspect.mockResolvedValue({ permission: "edit", files: true, commands: false, power: false })
  files.mockResolvedValue({ entries: [] })
  render(<RemoteWorkspace environment={environment} onClose={() => {}} />)
  expect(await screen.findByLabelText("Upload files")).toBeInTheDocument()
  expect(screen.queryByRole("tab", { name: "Terminal" })).not.toBeInTheDocument()
  expect(power).not.toHaveBeenCalled()
})
it("provides a reconnect action when revoked or expired", async () => {
  inspect.mockRejectedValueOnce(new Error("Session expired or revoked")).mockResolvedValue({ permission: "view", files: false, commands: false, power: false })
  reconnect.mockResolvedValue({})
  refreshPlatform.mockResolvedValue(undefined)
  render(<RemoteWorkspace environment={environment} onClose={() => {}} />)
  expect(await screen.findByRole("alert")).toHaveTextContent("Session expired or revoked")
  fireEvent.click(screen.getByRole("button", { name: "Reconnect" }))
  await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument())
  expect(reconnect).toHaveBeenCalledExactlyOnceWith("remote")
  expect(refreshPlatform).toHaveBeenCalledOnce()
})
it("reads shared files with the file API instead of executing guest commands", async () => {
  files.mockImplementation((_id, request) => Promise.resolve(request.operation === "list" ? { entries: [{ name: "readme.txt", directory: false, size: 5 }] } : { data: "aGVsbG8=" }))
  render(<RemoteFiles environmentId="remote" writable={false} />)
  fireEvent.click(await screen.findByRole("button", { name: "readme.txt" }))
  await waitFor(() => expect(screen.getByLabelText("Shared file contents")).toHaveValue("hello"))
  expect(files).toHaveBeenCalledWith("remote", { operation: "read", path: "readme.txt", offset: 0, length: 5 })
})
