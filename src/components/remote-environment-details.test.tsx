// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { RemoteEnvironmentDetails } from "./remote-environment-details"
import type { Environment } from "@/types/platform"
const { inspect, reconnect, refreshPlatform } = vi.hoisted(() => ({ inspect: vi.fn(), reconnect: vi.fn(), refreshPlatform: vi.fn() }))
vi.mock("@/api/remote-access-api", () => ({ remoteAccessApi: { inspect, reconnect } }))
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ deleteEnvironment: vi.fn(), refreshPlatform }) }))
vi.mock("./sharing-panel", () => ({ SharingPanel: ({ reconnectEnvironmentId, onConnected }: { reconnectEnvironmentId: string; onConnected(): void }) => <button data-node={reconnectEnvironmentId} onClick={onConnected}>Update connection</button> }))
afterEach(() => { cleanup(); vi.resetAllMocks() })
it("reconnects this node and replaces its stale error with current permissions", async () => {
  inspect.mockRejectedValueOnce(new Error("The tunnel is unavailable")).mockResolvedValueOnce({ permission: "view", files: true, commands: false, desktop: false, power: false })
  reconnect.mockResolvedValue({})
  refreshPlatform.mockResolvedValue(undefined)
  const environment = { id: "env-existing", name: "Remote project", status: "error", runtime: "shared://tunnel/old/share-old" } as Environment
  render(<RemoteEnvironmentDetails environment={environment} onOpenChange={() => {}} onOpenEnvironment={() => {}} />)
  expect(await screen.findByRole("alert")).toHaveTextContent("The tunnel is unavailable")
  fireEvent.click(screen.getByRole("button", { name: "Reconnect" }))
  await waitFor(() => expect(screen.getByText("Allowed")).toBeInTheDocument())
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  expect(inspect).toHaveBeenCalledTimes(2)
  expect(inspect).toHaveBeenLastCalledWith("env-existing")
  expect(reconnect).toHaveBeenCalledExactlyOnceWith("env-existing")
  expect(refreshPlatform).toHaveBeenCalledOnce()
})
it("keeps manual connection available when saved credentials are missing", async () => {
  inspect.mockRejectedValue(new Error("Session expired"))
  reconnect.mockRejectedValue(new Error("No saved recipient password. Enter the share details again."))
  const environment = { id: "env-existing", name: "Remote project", status: "error", runtime: "shared://tunnel/old/share-old" } as Environment
  render(<RemoteEnvironmentDetails environment={environment} onOpenChange={() => {}} onOpenEnvironment={() => {}} />)
  expect(await screen.findByRole("alert")).toHaveTextContent("Session expired")
  fireEvent.click(screen.getByRole("button", { name: "Reconnect" }))
  expect(await screen.findByRole("alert")).toHaveTextContent("No saved recipient password")
  expect(screen.getByRole("button", { name: "Update connection" })).toBeInTheDocument()
})
