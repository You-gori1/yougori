// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { AppCloseDialog } from "./app-close-dialog"
const { invoke, listener, nativeListener, unlisten, nativeUnlisten } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listener: { current: undefined as (() => void) | undefined },
  nativeListener: { current: undefined as (() => void) | undefined },
  unlisten: vi.fn(), nativeUnlisten: vi.fn(),
}))
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke }))
vi.mock("@tauri-apps/api/event", () => ({ listen: (_event: string, callback: () => void) => { listener.current = callback; return Promise.resolve(unlisten) } }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ listen: (_event: string, callback: () => void) => { nativeListener.current = callback; return Promise.resolve(nativeUnlisten) } }) }))
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ state: { environments: [{ id: "one", name: "My environment", status: "running" }] } }) }))
beforeEach(() => { vi.resetAllMocks(); vi.stubGlobal("PointerEvent", MouseEvent); invoke.mockResolvedValue(undefined) })
afterEach(() => { cleanup(); vi.unstubAllGlobals() })
async function openDialog() {
  render(<AppCloseDialog />)
  await act(async () => { listener.current!() })
  return screen.findByRole("dialog")
}
it("offers background mode alongside full shutdown", async () => {
  await openDialog()
  expect(screen.getByRole("button", { name: "Close app, keep environments running" })).toBeEnabled()
  fireEvent.click(screen.getByRole("button", { name: "Stop environments and quit" }))
  await waitFor(() => expect(invoke).toHaveBeenCalledExactlyOnceWith("finish_app_close", { keepRunning: false }))
})
it("closes the dashboard without stopping environments and can be used again after reopening", async () => {
  await openDialog()
  fireEvent.click(screen.getByRole("button", { name: "Close app, keep environments running" }))
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  expect(invoke).toHaveBeenCalledExactlyOnceWith("finish_app_close", { keepRunning: true })
  await act(async () => { listener.current!() })
  expect(screen.getByRole("dialog", { name: "Environments are still active" })).toBeVisible()
  fireEvent.click(screen.getByRole("button", { name: "Stop environments and quit" }))
  await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("finish_app_close", { keepRunning: false }))
  expect(invoke).toHaveBeenCalledTimes(2)
})

it("keeps background close errors visible and allows retrying", async () => {
  invoke.mockRejectedValueOnce(new Error("Could not hide the dashboard"))
  await openDialog()
  fireEvent.click(screen.getByRole("button", { name: "Close app, keep environments running" }))
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not hide the dashboard")
  fireEvent.click(screen.getByRole("button", { name: "Close app, keep environments running" }))
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  expect(invoke).toHaveBeenCalledTimes(2)
  expect(invoke).toHaveBeenLastCalledWith("finish_app_close", { keepRunning: true })
})

it("prevents repeated close requests while the windows are hiding", async () => {
  let finish!: () => void
  invoke.mockImplementation(() => new Promise<void>(resolve => { finish = resolve }))
  await openDialog()
  fireEvent.click(screen.getByRole("button", { name: "Close app, keep environments running" }))
  const progress = screen.getByRole("dialog", { name: "Closing Yougori…" })
  fireEvent.keyDown(progress, { key: "Escape" })
  await act(async () => { nativeListener.current!(); listener.current!() })
  expect(progress).toBeVisible()
  expect(screen.getByText("Closing the windows. Your environments will keep running.")).toBeVisible()
  expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull()
  expect(invoke).toHaveBeenCalledTimes(1)
  await act(async () => { finish() })
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
})
it("allows cancelling without stopping environments", async () => {
  await openDialog()
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
  expect(invoke).not.toHaveBeenCalled()
})
it("keeps shutdown errors visible so the user can retry", async () => {
  invoke.mockRejectedValue(new Error("Wait for environment creation to finish before quitting"))
  await openDialog()
  fireEvent.click(screen.getByRole("button", { name: "Stop environments and quit" }))
  expect(await screen.findByRole("alert")).toHaveTextContent("Wait for environment creation")
  expect(screen.getByRole("button", { name: "Cancel" })).toBeEnabled()
  invoke.mockResolvedValueOnce(undefined)
  fireEvent.click(screen.getByRole("button", { name: "Stop environments and quit" }))
  expect(await screen.findByRole("status")).toHaveTextContent("Closing Yougori")
  expect(invoke).toHaveBeenCalledTimes(2)
})

it("shows progress for a native close that does not need confirmation", async () => {
  render(<AppCloseDialog />)
  await act(async () => { nativeListener.current!() })
  expect(screen.getByRole("dialog", { name: "Closing Yougori…" })).toBeVisible()
  expect(screen.getByRole("status")).toHaveTextContent("Please wait")
  expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull()
  // The existing native close flow owns shutdown; observing it must not stop
  // environments a second time or invoke a window destruction command.
  expect(invoke).not.toHaveBeenCalled()
})

it("replaces initial progress with confirmation and tolerates either event order", async () => {
  render(<AppCloseDialog />)
  await act(async () => { nativeListener.current!(); listener.current!(); nativeListener.current!() })
  expect(screen.getByRole("dialog", { name: "Environments are still active" })).toBeVisible()
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  expect(invoke).not.toHaveBeenCalled()
})

it("keeps progress visible through slow shutdown and after exit is scheduled", async () => {
  let finish!: () => void
  invoke.mockImplementation(() => new Promise<void>(resolve => { finish = resolve }))
  await openDialog()
  fireEvent.click(screen.getByRole("button", { name: "Stop environments and quit" }))
  const progress = screen.getByRole("dialog", { name: "Closing Yougori…" })
  expect(progress).toBeVisible()
  fireEvent.keyDown(progress, { key: "Escape" })
  await act(async () => { nativeListener.current!(); listener.current!() })
  expect(progress).toBeVisible()
  expect(screen.queryByRole("button", { name: "Stop environments and quit" })).toBeNull()
  expect(screen.queryByRole("button", { name: "Close" })).toBeNull()
  expect(invoke).toHaveBeenCalledTimes(1)
  await act(async () => { finish() })
  expect(progress).toBeVisible()
  expect(screen.getByRole("status")).toHaveTextContent("Closing Yougori")
})

it("removes both event listeners when unmounted", async () => {
  const view = render(<AppCloseDialog />)
  view.unmount()
  await waitFor(() => {
    expect(unlisten).toHaveBeenCalledOnce()
    expect(nativeUnlisten).toHaveBeenCalledOnce()
  })
})
