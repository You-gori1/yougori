// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import type { PlatformState } from "@/types/platform"
import { platformApi } from "@/api/platform-api"
import { ConnectedFiles } from "./connected-files"

const state = { environments: [{ id: "cloud", name: "Cloud", kind: "cloud", status: "running" }, { id: "v1", name: "V1", kind: "container", status: "running" }], connections: [
  { id: "conn-1", sourceId: "v1", targetId: "cloud", active: true, direction: "bidirectional", permissions: ["data"], selectedFolders: [{ environmentId: "v1", path: "/" }] },
] } as unknown as PlatformState
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ state }) }))
afterEach(() => { cleanup(); vi.restoreAllMocks() })

it("shows a selected V1 folder in the cloud workspace and browses it through the connection", async () => {
  const request = vi.spyOn(platformApi, "requestConnectedFiles").mockImplementation(async (_id, operation) => operation.operation === "read"
    ? { data: btoa("test") }
    : { entries: operation.path === "_selected/0" ? [{ name: "project", directory: true, size: 0 }] : [{ name: "notes.txt", directory: false, size: 4 }] })
  render(<ConnectedFiles environmentId="cloud" active />)
  expect(await screen.findByRole("button", { name: "V1 · /" })).toBeInTheDocument()
  expect(request).toHaveBeenCalledWith("cloud", { connectionId: "conn-1", operation: "list", path: "_selected/0" })
  fireEvent.click(await screen.findByRole("button", { name: "project" }))
  await waitFor(() => expect(request).toHaveBeenCalledWith("cloud", { connectionId: "conn-1", operation: "list", path: "_selected/0/project" }))
  expect(await screen.findByRole("button", { name: "notes.txt" })).toBeInTheDocument()
  fireEvent.click(screen.getByRole("button", { name: "notes.txt" }))
  expect(await screen.findByRole("textbox", { name: "Shared file contents" })).toHaveValue("test")
  expect(request).toHaveBeenCalledWith("cloud", { connectionId: "conn-1", operation: "read", path: "_selected/0/project/notes.txt", length: 4 })
})
