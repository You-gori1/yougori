// @vitest-environment jsdom
import { StrictMode, type ReactNode } from "react"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { platformApi } from "@/api/platform-api"
import { workspaceApi, type HostShare } from "@/api/workspace-api"
import { sharingApi } from "@/api/sharing-api"
import { pcAccessLevel } from "@/lib/pc-access"
import IsolatedCliView from "./isolated-cli-view"

const { environments } = vi.hoisted(() => ({ environments: [{ id: "env-a", name: "Alpha", kind: "container", status: "running", runtime: "alpine" }] }))
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ state: { environments, connections: [] }, refreshPlatform: vi.fn() }) }))
vi.mock("@/api/platform-api", () => ({ platformApi: { openIsolatedCli: vi.fn(), grantIsolatedCliEnvironment: vi.fn() } }))
vi.mock("@/api/workspace-api", () => ({ workspaceApi: { services: vi.fn(), share: vi.fn(), unshare: vi.fn(), chooseFolders: vi.fn() } }))
vi.mock("@/api/sharing-api", () => ({ sharingApi: { revoke: vi.fn() } }))
// The browser test covers modal focus and dismissal; these tests exercise access operations.
vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: { open: boolean; children: ReactNode }) => open ? <>{children}</> : null,
  DialogPopup: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DialogHeader: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DialogTitle: ({ children }: { children: ReactNode }) => <h2>{children}</h2>,
  DialogDescription: ({ children }: { children: ReactNode }) => <p>{children}</p>,
}))
vi.mock("@/components/guest-workspace", () => ({ GuestWorkspace: ({ initialEnvironmentId, toolbarActions }: { initialEnvironmentId: string; toolbarActions?: ReactNode }) => <>{toolbarActions}<div data-testid="guest">{initialEnvironmentId}</div></> }))

const share = (id: string, path: string, readOnly: boolean): HostShare => ({ id, environmentId: "cli-1", path, readOnly, mountPath: `/yougori/shared/my-pc/${id}`, guestUrl: "" })
const services = (shares: HostShare[]) => ({ services: [], publications: [], shares, notice: "" })
const props = { onHide: vi.fn() }
const access = () => screen.getByRole("region", { name: "My PC access" })

beforeEach(() => {
  vi.mocked(platformApi.openIsolatedCli).mockResolvedValue("cli-1")
  vi.mocked(platformApi.grantIsolatedCliEnvironment).mockResolvedValue({ grant: { id: "grant-1" }, command: "yougori exec env-a -- sh -lc 'pwd'" })
  vi.mocked(workspaceApi.services).mockResolvedValue(services([]))
  vi.mocked(workspaceApi.share).mockImplementation(async (_environmentId, path, readOnly) => share(`share-${path}`, path, readOnly))
  vi.mocked(workspaceApi.unshare).mockResolvedValue(undefined)
})
afterEach(() => { cleanup(); vi.resetAllMocks(); environments.splice(1) })

describe("Isolated CLI workspace", () => {
  it("names the PC permission levels the rest of Yougori uses", () => {
    expect(pcAccessLevel([])).toBe("No Access")
    expect(pcAccessLevel([share("a", "C:\\Projects", true)])).toBe("View Only")
    expect(pcAccessLevel([share("a", "C:\\Projects", true), share("b", "C:\\Notes", false)])).toBe("View & Edit")
  })

  it("starts the isolated microVM once and reports that it holds no PC access", async () => {
    render(<StrictMode><IsolatedCliView {...props} /></StrictMode>)
    expect((await screen.findByTestId("guest")).textContent).toBe("cli-1")
    expect(platformApi.openIsolatedCli).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole("combobox")).toBeNull()
    fireEvent.click(screen.getByRole("button", { name: "CLI access" }))
    expect(access().textContent).toContain("No Access")
  })

  it("grants selected PC folders at the chosen level and can return to No Access", async () => {
    vi.mocked(workspaceApi.chooseFolders).mockResolvedValue(["C:\\Projects"])
    render(<IsolatedCliView {...props} />)
    await screen.findByTestId("guest")
    fireEvent.click(screen.getByRole("button", { name: "CLI access" }))
    fireEvent.change(screen.getByRole("combobox", { name: "My PC permission for the isolated CLI" }), { target: { value: "edit" } })
    vi.mocked(workspaceApi.services).mockResolvedValue(services([share("share-1", "C:\\Projects", false)]))
    fireEvent.click(screen.getByRole("button", { name: "Choose PC folders" }))
    await waitFor(() => expect(access().textContent).toContain("View & Edit"))
    expect(workspaceApi.share).toHaveBeenCalledWith("cli-1", "C:\\Projects", false)
    expect((await screen.findByRole("status")).textContent).toContain("read, change, create, and delete")
    vi.mocked(workspaceApi.services).mockResolvedValue(services([]))
    fireEvent.click(screen.getByRole("button", { name: "Set No Access" }))
    await waitFor(() => expect(access().textContent).toContain("No Access"))
    expect(workspaceApi.unshare).toHaveBeenCalledWith("share-1")
  })

  it("grants one environment at a time and revokes that grant on request", async () => {
    render(<IsolatedCliView {...props} />)
    await screen.findByTestId("guest")
    fireEvent.click(screen.getByRole("button", { name: "CLI access" }))
    expect(screen.queryByRole("button", { name: "Revoke" })).toBeNull()
    fireEvent.change(screen.getByRole("combobox", { name: "Environment for isolated CLI" }), { target: { value: "env-a" } })
    fireEvent.click(screen.getByRole("button", { name: "Grant 24h" }))
    await waitFor(() => expect(platformApi.grantIsolatedCliEnvironment).toHaveBeenCalledWith("env-a", "view"))
    fireEvent.click(await screen.findByRole("button", { name: "Revoke" }))
    await waitFor(() => expect(sharingApi.revoke).toHaveBeenCalledWith("grant-1"))
    await waitFor(() => expect(screen.queryByRole("button", { name: "Revoke" })).toBeNull())
  })
  it("grants all current environments and revokes every resulting grant", async () => {
    environments.push({ ...environments[0]!, id: "env-b", name: "Beta" })
    vi.mocked(platformApi.grantIsolatedCliEnvironment).mockImplementation(async id => ({ grant: { id: `grant-${id}` }, command: `yougori exec ${id}` }))
    render(<IsolatedCliView {...props} />)
    await screen.findByTestId("guest")
    fireEvent.click(screen.getByRole("button", { name: "CLI access" }))
    fireEvent.change(screen.getByRole("combobox", { name: "Environment for isolated CLI" }), { target: { value: "all" } })
    fireEvent.change(screen.getByRole("combobox", { name: "Isolated CLI permission" }), { target: { value: "control" } })
    fireEvent.click(screen.getByRole("button", { name: "Grant 24h" }))
    await waitFor(() => expect(platformApi.grantIsolatedCliEnvironment).toHaveBeenCalledTimes(2))
    expect(platformApi.grantIsolatedCliEnvironment).toHaveBeenCalledWith("env-a", "control")
    expect(platformApi.grantIsolatedCliEnvironment).toHaveBeenCalledWith("env-b", "control")
    fireEvent.click(await screen.findByRole("button", { name: "Revoke" }))
    await waitFor(() => expect(sharingApi.revoke).toHaveBeenCalledTimes(2))
    expect(sharingApi.revoke).toHaveBeenCalledWith("grant-env-a")
    expect(sharingApi.revoke).toHaveBeenCalledWith("grant-env-b")
  })
  it("keeps successful grants revocable when an all-environments grant partially fails", async () => {
    environments.push({ ...environments[0]!, id: "env-b", name: "Beta" })
    vi.mocked(platformApi.grantIsolatedCliEnvironment).mockResolvedValueOnce({ grant: { id: "partial" }, command: "ready" }).mockRejectedValueOnce(new Error("Beta unavailable"))
    render(<IsolatedCliView {...props} />)
    await screen.findByTestId("guest")
    fireEvent.click(screen.getByRole("button", { name: "CLI access" }))
    fireEvent.change(screen.getByRole("combobox", { name: "Environment for isolated CLI" }), { target: { value: "all" } })
    fireEvent.click(screen.getByRole("button", { name: "Grant 24h" }))
    expect((await screen.findByRole("alert")).textContent).toContain("Beta unavailable")
    expect(screen.getByRole("status").textContent).toContain("1 of 2 grants created")
    fireEvent.click(screen.getByRole("button", { name: "Revoke" }))
    await waitFor(() => expect(sharingApi.revoke).toHaveBeenCalledWith("partial"))
  })

})
