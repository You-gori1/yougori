// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"
import { RemoteWorkspaceToolbar } from "./remote-workspace-toolbar"
import type { Environment } from "@/types/platform"

vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ openEnvironmentWindow: vi.fn().mockResolvedValue(true) }) }))
vi.mock("./connection-skills-dialog", () => ({ ConnectionSkillsDialog: () => <button>Skills</button> }))
vi.mock("./guest-app-launcher", () => ({ GuestAppLauncher: () => <button>Apps</button> }))
afterEach(cleanup)

it("offers the shared tools and selects a dedicated installer without running a power action", async () => {
  const install = vi.fn()
  render(<RemoteWorkspaceToolbar environment={{ id: "remote", status: "running" } as Environment} capabilities={{ permission: "control", files: true, commands: true, power: true, desktop: false, skills: true, apps: true, installers: true, internet: true }} onInstall={install} onView={() => {}} onClose={() => {}} onError={() => {}} />)
  for (const name of ["Skills", "Apps", "Install tools", "New window", "Switch window", "Toggle fullscreen", "Workspace actions"]) expect(screen.getByRole("button", { name })).toBeEnabled()
  fireEvent.keyDown(screen.getByRole("button", { name: "Install tools" }), { key: "ArrowDown" })
  fireEvent.click(await screen.findByRole("menuitem", { name: "Install Codex" }))
  expect(install).toHaveBeenCalledExactlyOnceWith("codex")
})
