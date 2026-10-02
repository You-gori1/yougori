// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { cleanup, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { ProjectManager } from "./project-manager"
import { EnvironmentChanges } from "./environment-changes"
import { ModelChat } from "./model-workspace"
import { projectsApi, changesApi, modelsApi } from "@/api/projects-api"

vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ refreshPlatform: vi.fn().mockResolvedValue(undefined), state: { environments: [] } }) }))
vi.mock("@/api/projects-api", () => ({ projectsApi: { discover: vi.fn(), inspect: vi.fn(), importCompose: vi.fn(), action: vi.fn(), choose: vi.fn(), status: vi.fn() }, changesApi: { inspect: vi.fn() }, modelsApi: { status: vi.fn(), chat: vi.fn(), run: vi.fn() } }))
const preview = { path: "C:/project/yougori.yaml", project: "my-app", environments: [{ name: "frontend", type: "container", image: "node:24", cpu: 2, memoryGb: 4, gpu: false, pcAccess: 1, editPc: true, variables: ["TOKEN"], action: "create" }], connections: 1, publications: 0 }
afterEach(cleanup)
beforeEach(() => { vi.clearAllMocks(); vi.mocked(projectsApi.discover).mockResolvedValue([]) })
describe("project workflows", () => {
  it("reports a failed public HTTPS probe independently of a running application", async () => {
    vi.mocked(projectsApi.discover).mockResolvedValue([preview])
    vi.mocked(projectsApi.inspect).mockResolvedValue(preview)
    vi.mocked(projectsApi.status).mockResolvedValue({ project: "my-app", status:"notReady",saved:true, ready: false, environments: {frontend: {environmentId:"env-test",level:"localApplication",ready:false,applicationVerified:true,verifiedPublicly:false,stages:{running:{status:"ready"},localHttp:{status:"ready",httpStatus:200},publicHttps:{status:"failed",httpStatus:530}},recoveryAction:"Inspect the public HTTPS stage"}} })
    render(<ProjectManager />)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", {name:"Projects"}))
    await user.click(await screen.findByRole("button", {name:/my-app C:\/project/}))
    await user.click(await screen.findByRole("button", {name:"Check readiness"}))
    const report=await screen.findByLabelText("Deployment readiness")
    expect(report).toHaveTextContent("Deployment needs attention")
    expect(report).toHaveTextContent("Local HTTP: ready (HTTP 200)")
    expect(report).toHaveTextContent("Public HTTPS: failed (HTTP 530)")
    expect(report).toHaveTextContent("Inspect the public HTTPS stage")
  })
  it("does not claim application verification for a runtime without a health check", async () => {
    vi.mocked(projectsApi.discover).mockResolvedValue([preview])
    vi.mocked(projectsApi.inspect).mockResolvedValue(preview)
    vi.mocked(projectsApi.status).mockResolvedValue({ project:"my-app",status:"ready",saved:true,ready:true,environments:{frontend:{environmentId:"env-test",level:"runtime",ready:true,applicationVerified:false,verifiedPublicly:false,stages:{process:{status:"unverified"}}}} })
    render(<ProjectManager />)
    const user=userEvent.setup()
    await user.click(screen.getByRole("button",{name:"Projects"}))
    await user.click(await screen.findByRole("button",{name:/my-app C:\/project/}))
    await user.click(await screen.findByRole("button",{name:"Check readiness"}))
    expect(await screen.findByLabelText("Deployment readiness")).toHaveTextContent("Runtime ready; application verification not configured")
    expect(screen.queryByText("Application verified")).not.toBeInTheDocument()
  })
  it("previews Compose and writes YAML only when import is clicked", async () => {
    vi.mocked(projectsApi.choose).mockResolvedValue("C:/project/compose.yaml")
    vi.mocked(projectsApi.importCompose).mockResolvedValue(preview)
    render(<ProjectManager />)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Projects" }))
    await user.click(screen.getByRole("button", { name: "Import Docker Project" }))
    expect(await screen.findByText("frontend")).toBeInTheDocument()
    expect(projectsApi.importCompose).toHaveBeenCalledWith("C:/project/compose.yaml", false)
    expect(projectsApi.action).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Create yougori.yaml" }))
    await waitFor(() => expect(projectsApi.importCompose).toHaveBeenCalledWith("C:/project/compose.yaml", true))
    expect(await screen.findByRole("button", { name: "Apply & start" })).toBeEnabled()
    expect(screen.getByText(/View & Edit access/)).toBeInTheDocument()
  })
  it("shows exact counts and a real diff and confirms baseline replacement", async () => {
    vi.mocked(changesApi.inspect).mockResolvedValue({ available: true, baselineAt: "2026-09-20T12:00:00Z", summary: { modified: 23, created: 4, deleted: 2, renamed: 1, variables: 3, packagesInstalled: 6, packagesChanged: 6, configuration: 1 }, files: [{ folder: "C:/project", path: "app.ts", kind: "modified", beforeBytes: 7, afterBytes: 6, diff: "--- a/app.ts\n+++ b/app.ts\n-before\n+after\n" }], variables: [{ name: "TOKEN", before: "[redacted]", after: "[redacted]" }] })
    render(<EnvironmentChanges environmentId="env-test" />)
    const user = userEvent.setup()
    await user.click(screen.getByRole("button", { name: "Changes" }))
    const counts = await screen.findByLabelText("Change counts")
    expect(within(counts).getByText("23")).toBeInTheDocument()
    expect(screen.getByText("app.ts")).toBeInTheDocument()
    await user.click(screen.getByText("app.ts"))
    expect(screen.getByText("-before")).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "New baseline" }))
    expect(changesApi.inspect).not.toHaveBeenCalledWith("env-test", true, 0)
    await user.click(screen.getByRole("button", { name: "Save new baseline" }))
    await waitFor(() => expect(changesApi.inspect).toHaveBeenCalledWith("env-test", true, 0))
  })
  it("waits for model readiness and sends the conversation to the selected environment", async () => {
    vi.mocked(modelsApi.status).mockResolvedValue({ status: "ready", model: "TinyLlama/test", error: null })
    vi.mocked(modelsApi.chat).mockResolvedValue({ choices: [{ message: { content: "Hello from your GPU" } }] })
    render(<ModelChat environmentId="env-model" />)
    const user = userEvent.setup()
    await screen.findByText(/TinyLlama\/test/)
    await user.type(screen.getByRole("textbox", { name: "Message your model" }), "Hello")
    await user.click(screen.getByRole("button", { name: "Send message" }))
    expect(await screen.findByText("Hello from your GPU")).toBeInTheDocument()
    expect(modelsApi.chat).toHaveBeenCalledWith("env-model", [{ role: "user", content: "Hello" }], { maxTokens: 1024, temperature: 0.7 })
  })
})
