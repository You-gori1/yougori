// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { EnvironmentDownloadDialog } from "./environment-download-dialog"
import type { Environment } from "@/types/platform"

const { list, start, stop, status, services, writeText } = vi.hoisted(() => ({ list: vi.fn(), start: vi.fn(), stop: vi.fn(), status: vi.fn(), services: vi.fn(), writeText: vi.fn() }))
vi.mock("@/api/environment-download-api", () => ({ environmentDownloadApi: { list, start, stop } }))
vi.mock("@/api/workspace-api", () => ({ workspaceApi: { services } }))
vi.mock("@/lib/terminal-clipboard", () => ({ terminalClipboard: { writeText } }))
const environment = { id: "env-one", name: "My CRM", kind: "container", provider: "yougoriOci", status: "stopped" } as Environment
const fixtureState = {
  environments: [], savedDomains: [{ id: "domain-1", hostname: "copies.example.com" }, { id: "domain-2", hostname: "busy.example.com" }],
}
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ setEnvironmentStatus: status, state: fixtureState }) }))
const link = { environmentId: "env-one", active: true, url: "https://example.com/temporary", domain: null, sizeBytes: 1e9, downloads: 7 }
beforeEach(() => { vi.stubGlobal("PointerEvent", MouseEvent); vi.clearAllMocks(); list.mockResolvedValue([{ ...link, active: false, url: null }]); start.mockResolvedValue(link); stop.mockResolvedValue(undefined); services.mockResolvedValue({ publications: [] }); status.mockResolvedValue(undefined); writeText.mockResolvedValue(undefined) })
afterEach(() => { cleanup(); vi.unstubAllGlobals() })
const mount = (value = environment) => render(<EnvironmentDownloadDialog environment={value} onClose={vi.fn()} />)

it("requires review and creates a quick link while showing lifetime counts", async () => {
  mount(); const create = screen.getByRole("button", { name: "Create download link" });
  await waitFor(() => expect(screen.getByText("Downloads (all time): 7")).toBeInTheDocument());
  expect(create).toBeDisabled(); expect(screen.getByRole("switch", { name: "Download link" })).toHaveAttribute("aria-disabled", "true");
  fireEvent.click(screen.getByRole("checkbox")); fireEvent.click(create);
  await screen.findByDisplayValue(link.url);
  expect(start).toHaveBeenCalledExactlyOnceWith("env-one", undefined); expect(status).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Copy link" }));
  await screen.findByRole("button", { name: "Copied" }); expect(writeText).toHaveBeenCalledWith(link.url);
})

it("stops a running environment before copying and uses the selected saved domain", async () => {
  mount({ ...environment, status: "running" });
  await waitFor(() => expect(screen.queryByText("Download link: Checking…")).not.toBeInTheDocument());
  fireEvent.change(screen.getByRole("combobox", { name: "Link address" }), { target: { value: "copies.example.com" } });
  fireEvent.click(screen.getByRole("checkbox")); fireEvent.click(screen.getByRole("button", { name: "Stop and create download link" }));
  await screen.findByDisplayValue(link.url);
  expect(status).toHaveBeenCalledExactlyOnceWith("env-one", "stopped"); expect(start).toHaveBeenCalledWith("env-one", "copies.example.com");
  expect(status.mock.invocationCallOrder[0]!).toBeLessThan(start.mock.invocationCallOrder[0]!);
})

it("turns off an existing link without resetting the lifetime count", async () => {
  list.mockResolvedValue([link]); mount(); await screen.findByDisplayValue(link.url);
  fireEvent.click(screen.getByRole("switch", { name: "Download link" }));
  await screen.findByText("Download link: Off"); expect(stop).toHaveBeenCalledWith("env-one");
  expect(screen.getByText("Downloads (all time): 7")).toBeInTheDocument(); expect(start).not.toHaveBeenCalled();
})

it("does not publish after a failed stop and allows retry", async () => {
  status.mockRejectedValueOnce(new Error("Guest refused to stop")); mount({ ...environment, status: "running" });
  await waitFor(() => expect(screen.queryByText("Download link: Checking…")).not.toBeInTheDocument());
  fireEvent.click(screen.getByRole("checkbox")); fireEvent.click(screen.getByRole("button", { name: "Stop and create download link" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Guest refused to stop"); expect(start).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Stop and create download link" })); await screen.findByDisplayValue(link.url);
})

it("disables domains currently used by another download and does not allow duplicate starts", async () => {
  list.mockResolvedValue([{ ...link, active: false, url: null }, { ...link, environmentId: "env-other", domain: "busy.example.com" }]);
  let done!: (value: typeof link) => void; start.mockImplementation(() => new Promise(resolve => { done = resolve }));
  mount(); await waitFor(() => expect(screen.getByRole("option", { name: "busy.example.com · In use" })).toBeDisabled());
  fireEvent.click(screen.getByRole("checkbox")); const create = screen.getByRole("button", { name: "Create download link" });
  fireEvent.click(create); fireEvent.click(create); expect(start).toHaveBeenCalledTimes(1);
  await act(async () => { done(link) }); await screen.findByDisplayValue(link.url);
})
