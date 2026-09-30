// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, expect, it } from "vitest"
import { TerminalLinkCards } from "./terminal-link-cards"

afterEach(cleanup)

it("closes all current QR codes, keeps them dismissed on rescan, and allows new links", () => {
  const links = ["https://example.com/one", "https://example.com/two"]
  const { rerender } = render(<TerminalLinkCards links={links} />)
  expect(screen.getAllByRole("img")).toHaveLength(2)
  fireEvent.click(screen.getByRole("button", { name: "Close all" }))
  expect(screen.queryByRole("region", { name: "Terminal link QR codes" })).toBeNull()
  rerender(<TerminalLinkCards links={[...links]} />)
  expect(screen.queryByRole("img")).toBeNull()
  rerender(<TerminalLinkCards links={[...links, "https://example.com/new"]} />)
  expect(screen.getAllByRole("img")).toHaveLength(1)
  expect(screen.getByRole("img", { name: "QR code for https://example.com/new" })).toBeTruthy()
})

it("preserves individual dismissals when closing the remaining cards", () => {
  const links = ["https://example.com/one", "https://example.com/two"]
  const { rerender } = render(<TerminalLinkCards links={links} />)
  fireEvent.click(screen.getByRole("button", { name: `Dismiss QR code for ${links[0]}` }))
  expect(screen.getAllByRole("img")).toHaveLength(1)
  rerender(<TerminalLinkCards links={[links[1]!]} />)
  fireEvent.click(screen.getByRole("button", { name: "Close all" }))
  rerender(<TerminalLinkCards links={links} />)
  expect(screen.queryByRole("img")).toBeNull()
})
