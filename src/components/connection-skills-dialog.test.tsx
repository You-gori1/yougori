// @vitest-environment jsdom
import { StrictMode } from "react"
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { platformApi, type InstalledSkills } from "@/api/platform-api"
import { ConnectionSkillsDialog } from "./connection-skills-dialog"

const { environment } = vi.hoisted(() => ({ environment: { id: "env-a", name: "Alpha", kind: "container", status: "running", runtime: "alpine" } }))
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ state: { environments: [environment], connections: [] } }) }))
vi.mock("@/api/platform-api", () => ({ platformApi: { connectionSkills: vi.fn(), installEnvironmentSkills: vi.fn() } }))
vi.mock("@/lib/terminal-clipboard", () => ({ terminalClipboard: { writeText: vi.fn() } }))
const installed: InstalledSkills = { environmentId: "env-a", skillPath: "/guest/yougori-environment/SKILL.md", referencePath: "/guest/yougori-environment/references/connections.md", delivery: "directory", message: "Skills are inside this environment." }
beforeEach(() => {
  environment.status = "running"
  vi.mocked(platformApi.installEnvironmentSkills).mockResolvedValue(installed)
  vi.mocked(platformApi.connectionSkills).mockResolvedValue('```json\n{"summary":{"connectedNodes":0,"connections":0,"ready":0,"blocked":0},"sourceStatus":"running","connections":[]}\n```')
})
afterEach(() => { cleanup(); vi.resetAllMocks() })

describe("Skills installation", () => {
  it("installs once on click under StrictMode, exposes both paths, and keeps the bundle when reopened", async () => {
    render(<StrictMode><ConnectionSkillsDialog environmentId="env-a" /></StrictMode>)
    expect(platformApi.installEnvironmentSkills).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Connection skills" }))
    expect((await screen.findByRole("textbox", { name: "Installed skill path" }) as HTMLInputElement).value).toBe(installed.skillPath)
    expect((screen.getByRole("textbox", { name: "Installed connection reference path" }) as HTMLInputElement).value).toBe(installed.referencePath)
    expect(platformApi.installEnvironmentSkills).toHaveBeenCalledExactlyOnceWith("env-a")
    fireEvent.click(screen.getByRole("button", { name: "Close" }))
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
    fireEvent.click(screen.getByRole("button", { name: "Connection skills" }))
    await screen.findByRole("textbox", { name: "Installed skill path" })
    expect(platformApi.installEnvironmentSkills).toHaveBeenCalledTimes(1)
  })

  it("prevents duplicate installations and reports a failed transfer without claiming success", async () => {
    let fail!: (reason: Error) => void
    vi.mocked(platformApi.installEnvironmentSkills).mockReturnValue(new Promise((_, reject) => { fail = reject }))
    render(<ConnectionSkillsDialog environmentId="env-a" />)
    fireEvent.click(screen.getByRole("button", { name: "Connection skills" }))
    expect((screen.getByRole("button", { name: "Install skill files" }) as HTMLButtonElement).disabled).toBe(true)
    await act(async () => fail(new Error("Guest disk is full")))
    expect((await screen.findByRole("alert")).textContent).toContain("installation was not confirmed")
    expect(screen.queryByRole("textbox", { name: "Installed skill path" })).toBeNull()
    vi.mocked(platformApi.installEnvironmentSkills).mockResolvedValue(installed)
    fireEvent.click(screen.getByRole("button", { name: "Install skill files" }))
    await screen.findByRole("textbox", { name: "Installed skill path" })
    expect(platformApi.installEnvironmentSkills).toHaveBeenCalledTimes(2)
  })

  it("keeps stopped environments readable without attempting an installation", async () => {
    environment.status = "stopped"
    render(<ConnectionSkillsDialog environmentId="env-a" />)
    fireEvent.click(screen.getByRole("button", { name: "Connection skills" }))
    await screen.findByRole("textbox", { name: "AI agent connection instructions" })
    expect(screen.getByText(/Start this environment to install Skills/)).toBeTruthy()
    expect(platformApi.installEnvironmentSkills).not.toHaveBeenCalled()
  })
})
