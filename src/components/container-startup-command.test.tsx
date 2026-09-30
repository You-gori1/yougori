// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { ContainerStartupCommand } from "./container-startup-command"
import type { Environment } from "@/types/platform"

const environment = { id: "test", status: "stopped", containerCommand: "sleep 2147483647" } as Environment
afterEach(cleanup)

it("shows the saved command and reports edits as a draft without its own save button", () => {
  const onDraftChange = vi.fn()
  const view = render(<ContainerStartupCommand environment={environment} draft={null} disabled={false} onDraftChange={onDraftChange} />)
  expect((screen.getByLabelText("Startup command") as HTMLTextAreaElement).value).toBe("sleep 2147483647")
  expect(screen.queryByRole("button", { name: /save/i })).toBeNull()
  fireEvent.change(screen.getByLabelText("Startup command"), { target: { value: "cd /project\nexec npm start" } })
  expect(onDraftChange).toHaveBeenCalledExactlyOnceWith("cd /project\nexec npm start")
  view.rerender(<ContainerStartupCommand environment={{ ...environment, cpuUsage: 12 }} draft={"cd /project\nexec npm start"} disabled={false} onDraftChange={onDraftChange} />)
  expect((screen.getByLabelText("Startup command") as HTMLTextAreaElement).value).toBe("cd /project\nexec npm start")
})

it("explains that a running container must stop before a changed command is saved", () => {
  const props = { environment: { ...environment, status: "running" as const }, disabled: false, onDraftChange: vi.fn() }
  const view = render(<ContainerStartupCommand {...props} draft={null} />)
  expect(screen.queryByText(/Stop the container/)).toBeNull()
  view.rerender(<ContainerStartupCommand {...props} draft="exec server" />)
  expect(screen.getByText(/Stop the container, then Save changes/)).toBeTruthy()
})
