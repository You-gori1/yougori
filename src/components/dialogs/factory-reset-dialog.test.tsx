// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { FactoryResetDialog } from "./factory-reset-dialog"
import type { Environment } from "@/types/platform"

const reset = vi.hoisted(() => vi.fn())
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ factoryResetEnvironment: reset }) }))
const environment = { id: "test", name: "Windows", kind: "fullVm", provider: "qemu", status: "stopped", runtime: "Windows.iso" } as Environment
beforeEach(() => { reset.mockReset() })
afterEach(cleanup)

describe("factory reset confirmation", () => {
  it("requires the exact name, warns about reinstalling and waits for completion", async () => {
    let finish!: () => void
    reset.mockImplementation(() => new Promise<void>(resolve => { finish = resolve }))
    render(<FactoryResetDialog environment={environment} />)
    fireEvent.click(screen.getByRole("button", { name: "Factory reset" }))
    const confirm = await screen.findByRole("button", { name: "Erase data and reset" }) as HTMLButtonElement
    expect(confirm.disabled).toBe(true)
    expect(screen.getByText(/need to install the operating system again/)).toBeTruthy()
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "windows" } })
    expect(confirm.disabled).toBe(true)
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Windows" } })
    fireEvent.click(confirm)
    await waitFor(() => expect(reset).toHaveBeenCalledWith("test", "Windows"))
    expect(confirm.getAttribute("aria-busy")).toBe("true")
    expect((screen.getByRole("button", { name: "Cancel" }) as HTMLButtonElement).disabled).toBe(true)
    await act(async () => { finish() })
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull())
  })

  it("keeps the confirmation open on failure and allows retry", async () => {
    reset.mockRejectedValue(new Error("Original image is missing"))
    render(<FactoryResetDialog environment={environment} />)
    fireEvent.click(screen.getByRole("button", { name: "Factory reset" }))
    fireEvent.change(await screen.findByRole("textbox"), { target: { value: "Windows" } })
    fireEvent.click(screen.getByRole("button", { name: "Erase data and reset" }))
    expect((await screen.findByRole("alert")).textContent).toContain("Original image is missing")
    expect(screen.getByRole("alertdialog")).toBeTruthy()
    expect((screen.getByRole("button", { name: "Erase data and reset" }) as HTMLButtonElement).disabled).toBe(false)
  })

  it("disables reset for running environments and cancel never resets", async () => {
    const { rerender } = render(<FactoryResetDialog environment={{ ...environment, status: "running" }} />)
    expect((screen.getByRole("button", { name: "Factory reset" }) as HTMLButtonElement).disabled).toBe(true)
    rerender(<FactoryResetDialog environment={environment} />)
    fireEvent.click(screen.getByRole("button", { name: "Factory reset" }))
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }))
    expect(reset).not.toHaveBeenCalled()
  })
})
