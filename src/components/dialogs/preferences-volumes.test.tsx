// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { PreferencesVolumes } from "./preferences-volumes"
import type { VolumeListing } from "@/api/volumes-api"

const api = vi.hoisted(() => ({ list: vi.fn(), remove: vi.fn() }))
vi.mock("@/api/volumes-api", async importOriginal => ({ ...await importOriginal<typeof import("@/api/volumes-api")>(), volumesApi: api }))

const listing = (names: string[] = ["data", "old"]): VolumeListing => ({
  volumes: [
    { name: "data", inUse: true, mounts: [{ environmentId: "e1", environment: "db", target: "/data", readOnly: false, running: true }], stored: [{ location: "C:/Y", runtime: "containers", usedBy: ["db"], sizeBytes: 2048, sizeComplete: true }] },
    { name: "old", inUse: false, mounts: [], stored: [{ location: "C:/Y", runtime: "containers", usedBy: [], sizeBytes: 10, sizeComplete: false }] },
  ].filter(volume => names.includes(volume.name)) as VolumeListing["volumes"],
  checked: ["C:/Y"], problems: [], note: "",
})

beforeEach(() => { api.list.mockReset(); api.remove.mockReset() })
afterEach(cleanup)

describe("settings volumes", () => {
  it("lists users and sizes, and removes an unused volume only after confirming", async () => {
    api.list.mockResolvedValueOnce(listing()).mockResolvedValueOnce(listing(["data"]))
    api.remove.mockResolvedValue({ removed: "old", from: ["C:/Y"] })
    render(<PreferencesVolumes open />)
    expect(await screen.findByText("Used by db · 2.0 KB")).toBeTruthy()
    expect(screen.getByText("Unused · 10 B+")).toBeTruthy()
    expect(api.list).toHaveBeenCalledWith(false, false)
    expect(screen.getAllByRole("button", { name: "Remove" })).toHaveLength(1)
    fireEvent.click(screen.getByRole("button", { name: "Remove" }))
    expect(api.remove).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Delete" }))
    await waitFor(() => expect(api.remove).toHaveBeenCalledWith("old"))
    await waitFor(() => expect(screen.queryByText(/Unused/)).toBeNull())
  })

  it("checks stopped runtimes with sizes on request and shows failures", async () => {
    api.list.mockResolvedValueOnce(listing([])).mockResolvedValueOnce({ ...listing(["old"]), problems: ["D:/Y: drive offline"] })
    api.remove.mockRejectedValue(new Error("volume old is used by x"))
    render(<PreferencesVolumes open />)
    expect(await screen.findByText(/No volumes in running containers/)).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Check everything" }))
    await waitFor(() => expect(api.list).toHaveBeenLastCalledWith(true, true))
    expect(await screen.findByText("Not checked: D:/Y: drive offline")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Remove" }))
    fireEvent.click(screen.getByRole("button", { name: "Delete" }))
    expect((await screen.findByRole("alert")).textContent).toContain("used by x")
  })
})
