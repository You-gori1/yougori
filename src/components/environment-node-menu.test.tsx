// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { EnvironmentNodeMenu } from "./environment-node-menu"
import type { Environment } from "@/types/platform"
import type { ReactNode } from "react"

// Floating menu geometry needs a browser; exercise confirmation and errors in
// jsdom without its layout-less positioning/focus lifecycle.
vi.mock("@/components/ui/menu", () => ({
  MenuPopup: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  MenuItem: ({ children, onClick, disabled }: { children: ReactNode; onClick: () => void; disabled: boolean }) => <button disabled={disabled} onClick={onClick}>{children}</button>,
}))

const { remove } = vi.hoisted(() => ({ remove: vi.fn() }))
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ deleteEnvironment: remove, environmentActions: {} }) }))
afterEach(() => { cleanup(); vi.clearAllMocks(); vi.restoreAllMocks(); vi.unstubAllGlobals(); delete document.documentElement.dataset.yougoriNodeContext })
const environment = { id: "node-one", name: "My database", kind: "container", status: "stopped" } as Environment
async function openMenu(value = environment) {
  render(<EnvironmentNodeMenu environment={value}><div>Node surface</div></EnvironmentNodeMenu>)
  fireEvent.contextMenu(screen.getByText("Node surface"), { clientX: 100, clientY: 100 })
  fireEvent.click(screen.getByText("Delete node"))
  await screen.findByText(`Delete ${value.name}?`)
}
it("requires confirmation and cancellation leaves the node untouched", async () => {
  await openMenu()
  expect(screen.getByText("Delete My database?")).toBeInTheDocument()
  expect(remove).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
  await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument())
  expect(remove).not.toHaveBeenCalled()
})
it("closes immediately while deletion runs and allows retry after failure", async () => {
  let reject!: (error: Error) => void
  remove.mockImplementationOnce(() => new Promise<void>((_resolve, fail) => { reject = fail })).mockResolvedValueOnce(undefined)
  await openMenu()
  fireEvent.click(screen.getByRole("button", { name: "Delete node" }))
  await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument())
  expect(remove).toHaveBeenCalledExactlyOnceWith("node-one")
  fireEvent.contextMenu(screen.getByText("Node surface"))
  expect(screen.getByRole("button", { name: "Delete node" })).toBeDisabled()
  await act(async () => { reject(new Error("Runtime unavailable")) })
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument()
  await waitFor(() => expect(screen.getByRole("button", { name: "Delete node" })).toBeEnabled())
  fireEvent.click(screen.getByRole("button", { name: "Delete node" }))
  fireEvent.click(await screen.findByRole("button", { name: "Delete node" }))
  await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument())
  expect(remove).toHaveBeenCalledTimes(2)
})
it("explains that removing a cloud node keeps the remote server", async () => {
  await openMenu({ ...environment, kind: "cloud" })
  expect(screen.getByText(/remote server and its data are not deleted/)).toBeInTheDocument()
  expect(remove).not.toHaveBeenCalled()
})
it("preserves the Windows native context menu and confirms only its selected node", async () => {
  vi.stubGlobal("__TAURI_INTERNALS__", {})
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32")
  render(<EnvironmentNodeMenu environment={environment}><div>Node surface</div></EnvironmentNodeMenu>)
  const context = new MouseEvent("contextmenu", { bubbles: true, cancelable: true })
  fireEvent(screen.getByText("Node surface"), context)
  expect(context.defaultPrevented).toBe(false)
  expect(document.documentElement.dataset.yougoriNodeContext).toBe("node-one")
  expect(screen.queryByText("Delete node")).not.toBeInTheDocument()
  fireEvent(window, new CustomEvent("yougori-delete-node", { detail: "other-node" }))
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument()
  fireEvent(window, new CustomEvent("yougori-delete-node", { detail: "node-one" }))
  expect(await screen.findByRole("alertdialog")).toHaveTextContent("Delete My database?")
  expect(remove).not.toHaveBeenCalled()
})
