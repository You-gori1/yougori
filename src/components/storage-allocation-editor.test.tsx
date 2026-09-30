// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import { createRef } from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { StorageAllocationEditor } from "./storage-allocation-editor"
import { platformApi } from "@/api/platform-api"
import type { SectionSave } from "@/lib/resource-controls"
import type { Environment, StorageAllocation } from "@/types/platform"

vi.mock("@/api/platform-api", () => ({ platformApi: { getStorageAllocation: vi.fn(), expandEnvironmentStorage: vi.fn() } }))
const storage: StorageAllocation = { capacityGb: 64, physicalGb: 2, maximumGb: 200, shared: false }
const environment = { id: "test-vm", kind: "fullVm", status: "stopped", runtime: "windows.iso" } as Environment
beforeEach(() => { vi.resetAllMocks(); vi.mocked(platformApi.getStorageAllocation).mockResolvedValue(storage) })
afterEach(cleanup)

function renderEditor(target: Environment) {
  const handle = createRef<SectionSave>()
  const view = render(<StorageAllocationEditor environment={target} saveRef={handle} otherContainersActive />)
  return { handle, view }
}

describe("storage allocation", () => {
  it.each(["yougoriOci", "yougoriCuda"] as const)("stores a running %s container's own limit through Save changes", async provider => {
    vi.mocked(platformApi.getStorageAllocation).mockResolvedValue({ ...storage, limitEnforced: true })
    vi.mocked(platformApi.expandEnvironmentStorage).mockResolvedValue({ ...storage, capacityGb: 100, limitEnforced: true })
    const { handle } = renderEditor({ ...environment, provider, kind: "container", status: "running" })
    const slider = await screen.findByRole("slider", { name: "Storage limit" })
    expect(screen.getByText(/Used by this container: 2.00 GB/)).toBeTruthy()
    expect(screen.queryByRole("button", { name: /Save storage limit|Set storage limit|Expand storage/ })).toBeNull()
    expect(handle.current!.changed()).toBe(false)
    fireEvent.change(slider, { target: { value: "100" } })
    expect(handle.current!.changed()).toBe(true)
    expect(handle.current!.problem()).toBeNull()
    await act(() => handle.current!.save())
    expect(platformApi.expandEnvironmentStorage).toHaveBeenCalledWith(environment.id, 100)
    expect(handle.current!.changed()).toBe(false)
    expect(slider.getAttribute("min")).toBe("1")
    expect(slider.getAttribute("max")).toBe("200")
  })

  it("reduces a running container to 1 GB when its files fit", async () => {
    vi.mocked(platformApi.getStorageAllocation).mockResolvedValue({ ...storage, capacityGb: 20, physicalGb: 0.1, limitEnforced: true })
    vi.mocked(platformApi.expandEnvironmentStorage).mockResolvedValue({ ...storage, capacityGb: 1, physicalGb: 0.1, limitEnforced: true })
    const { handle } = renderEditor({ ...environment, kind: "container", status: "running" })
    await screen.findByRole("slider", { name: "Storage limit" })
    fireEvent.change(screen.getByRole("spinbutton", { name: "Storage size in GB" }), { target: { value: "1" } })
    await act(() => handle.current!.save())
    expect(platformApi.expandEnvironmentStorage).toHaveBeenCalledWith(environment.id, 1)
  })

  it("explains why a selected limit below current usage cannot be saved", async () => {
    vi.mocked(platformApi.getStorageAllocation).mockResolvedValue({ ...storage, capacityGb: 20, physicalGb: 9.27, limitEnforced: true })
    const { handle } = renderEditor({ ...environment, kind: "container", status: "running" })
    fireEvent.change(await screen.findByRole("slider"), { target: { value: "6" } })
    expect((await screen.findByRole("alert")).textContent).toContain("Choose at least 10 GB")
    expect(handle.current!.problem()).toContain("Choose at least 10 GB")
  })

  it("expands a stopped VM disk and never offers shrinking", async () => {
    vi.mocked(platformApi.expandEnvironmentStorage).mockResolvedValue({ ...storage, capacityGb: 100 })
    const { handle } = renderEditor(environment)
    const slider = await screen.findByRole("slider", { name: "Storage capacity" })
    expect(slider.getAttribute("min")).toBe("64")
    expect(slider.getAttribute("aria-valuetext")).toBe("64 GB")
    fireEvent.change(slider, { target: { value: "100" } })
    await act(() => handle.current!.save())
    expect(platformApi.expandEnvironmentStorage).toHaveBeenCalledWith("test-vm", 100)
    expect(slider.getAttribute("min")).toBe("100")
  })

  it("an untouched legacy container never blocks other changes; a moved slider must stop it once", async () => {
    vi.mocked(platformApi.getStorageAllocation).mockResolvedValue({ ...storage, limitEnforced: false })
    vi.mocked(platformApi.expandEnvironmentStorage).mockResolvedValue({ ...storage, limitEnforced: true })
    const running = { ...environment, kind: "container" as const, status: "running" as const }
    const handle = createRef<SectionSave>()
    const { rerender } = render(<StorageAllocationEditor environment={running} saveRef={handle} otherContainersActive />)
    const slider = await screen.findByRole("slider")
    expect(handle.current!.changed()).toBe(false)
    expect(screen.getByText(/Other containers can keep running/)).toBeTruthy()
    fireEvent.change(slider, { target: { value: "64" } })
    expect(handle.current!.problem()).toContain("Stop this container once")
    rerender(<StorageAllocationEditor environment={{ ...running, status: "stopped" }} saveRef={handle} otherContainersActive />)
    fireEvent.change(await screen.findByRole("slider"), { target: { value: "64" } })
    expect(handle.current!.problem()).toBeNull()
    await act(() => handle.current!.save())
    expect(platformApi.expandEnvironmentStorage).toHaveBeenCalledWith(environment.id, 64)
  })

  it("passes failures to Save changes without claiming that storage was expanded", async () => {
    vi.mocked(platformApi.expandEnvironmentStorage).mockRejectedValue(new Error("Disk locked"))
    const { handle } = renderEditor(environment)
    fireEvent.change(await screen.findByRole("slider"), { target: { value: "100" } })
    await expect(act(() => handle.current!.save())).rejects.toThrow("Disk locked")
    expect(handle.current!.changed()).toBe(true)
  })
})
