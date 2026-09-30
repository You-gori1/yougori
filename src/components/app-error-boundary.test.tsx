// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { AppErrorBoundary } from "./app-error-boundary"

afterEach(() => { cleanup(); vi.restoreAllMocks() })

it("preserves the normal interface when rendering succeeds", () => {
  render(<AppErrorBoundary><p>Environments ready</p></AppErrorBoundary>)
  expect(screen.getByText("Environments ready")).toBeTruthy()
  expect(screen.queryByRole("alert")).toBeNull()
})

it("shows a safe reload action after a render failure instead of a blank window", () => {
  vi.spyOn(console, "error").mockImplementation(() => undefined)
  const reload = vi.fn()
  function FailingView(): never { throw new Error("Unexpected renderer failure") }
  render(<AppErrorBoundary onReload={reload}><FailingView /></AppErrorBoundary>)
  expect(screen.getByRole("alert").textContent).toContain("does not delete or factory-reset environments")
  expect(screen.queryByText("Unexpected renderer failure")).toBeNull()
  fireEvent.click(screen.getByRole("button", { name: "Reload window" }))
  expect(reload).toHaveBeenCalledOnce()
})
