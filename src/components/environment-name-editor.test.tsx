// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import { createRef } from "react"
import { afterEach, expect, it, vi } from "vitest"
import { EnvironmentNameEditor } from "./environment-name-editor"
import type { SectionSave } from "@/lib/resource-controls"
import type { Environment } from "@/types/platform"

const rename = vi.hoisted(() => vi.fn())
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ renameEnvironment: rename }) }))
afterEach(() => { cleanup(); rename.mockReset() })
const environment = { id: "vm-1", name: "Windows 11", kind: "fullVm" } as Environment

it("keeps the draft during polling, has no own save button, and Enter submits the configuration", async () => {
  rename.mockResolvedValue(undefined)
  const handle = createRef<SectionSave>()
  const submit = vi.fn()
  const { rerender } = render(<EnvironmentNameEditor environment={environment} saveRef={handle} onSubmit={submit} />)
  expect(screen.queryByRole("button")).toBeNull()
  expect(handle.current!.changed()).toBe(false)
  fireEvent.change(screen.getByLabelText("VM name"), { target: { value: "  Work VM  " } })
  rerender(<EnvironmentNameEditor environment={{ ...environment }} saveRef={handle} onSubmit={submit} />)
  expect((screen.getByLabelText("VM name") as HTMLInputElement).value).toBe("  Work VM  ")
  fireEvent.keyDown(screen.getByLabelText("VM name"), { key: "Enter" })
  expect(submit).toHaveBeenCalledOnce()
  expect(handle.current!.changed()).toBe(true)
  await act(() => handle.current!.save())
  expect(rename).toHaveBeenCalledWith("vm-1", "Work VM")
})

it("reports invalid names as a problem and keeps the draft when saving fails", async () => {
  rename.mockRejectedValue(new Error("Could not save"))
  const handle = createRef<SectionSave>()
  render(<EnvironmentNameEditor environment={environment} saveRef={handle} />)
  fireEvent.change(screen.getByLabelText("VM name"), { target: { value: " " } })
  expect(handle.current!.problem()).toContain("2–80 characters")
  fireEvent.change(screen.getByLabelText("VM name"), { target: { value: "New VM" } })
  expect(handle.current!.problem()).toBeNull()
  await expect(act(() => handle.current!.save())).rejects.toThrow("Could not save")
  expect((screen.getByLabelText("VM name") as HTMLInputElement).value).toBe("New VM")
})
