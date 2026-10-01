// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { NeocloudForm } from "./neocloud-form"
import { runpodApi, type RunpodCatalog } from "@/api/runpod-api"

vi.mock("@/api/runpod-api", async importOriginal => ({
  ...await importOriginal<typeof import("@/api/runpod-api")>(),
  runpodApi: {
    status: vi.fn(), connect: vi.fn(), disconnect: vi.fn(), catalog: vi.fn(), template: vi.fn(), searchTemplates: vi.fn(), hub: vi.fn(), hubRepo: vi.fn(),
    createPod: vi.fn(), createEndpoint: vi.fn(), resources: vi.fn(), attach: vi.fn(), volume: vi.fn(), registry: vi.fn(),
  },
}))
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ state: { environments: [{ name: "taken" }] }, refreshPlatform: async () => undefined }) }))
vi.mock("@/api/workspace-api", () => ({ workspaceApi: { openUrl: vi.fn() } }))
vi.mock("@/components/ui/dialog", () => ({ DialogPanel: ({ children }: { children: React.ReactNode }) => <div>{children}</div> }))
vi.mock("@/components/ui/toast", () => ({ toastManager: { add: vi.fn() } }))

const account = { email: "me@example.com", balance: 10, spendPerHour: 0, spendLimit: 80 }
const catalog: RunpodCatalog = {
  checkedAt: "2026-09-27T00:00:00Z", account, issues: [], registries: [],
  gpus: [
    { id: "NVIDIA GeForce RTX 4090", name: "RTX 4090", vramGb: 24, securePrice: 0.74, communityPrice: 0.34, available: true, stock: "low", locations: [{ id: "US-KS-2", stock: "low" }], amd: false },
    { id: "NVIDIA A40", name: "A40", vramGb: 48, securePrice: 0.49, communityPrice: null, available: false, stock: "none", locations: [], amd: false },
  ],
  locations: [{ id: "US-KS-2", country: "United States" }],
  templates: [
    { id: "runpod-torch-v280", name: "Runpod Pytorch 2.8.0", image: "runpod/pytorch:1.0.2-cu1281-torch280-ubuntu2404", kind: "gpu", containerDiskGb: 30, volumeGb: 50, mountPath: "/workspace", own: false, official: true },
    { id: "runpod-ubuntu-2404", name: "Runpod Ubuntu 24.04", image: "runpod/base:1.0.2-ubuntu2404", kind: "cpu", containerDiskGb: 10, volumeGb: null, mountPath: "/workspace", own: false, official: true },
  ],
  volumes: [],
}

beforeEach(() => {
  vi.stubGlobal("PointerEvent", MouseEvent)
  vi.resetAllMocks()
  vi.mocked(runpodApi.status).mockResolvedValue({ installed: true, connected: true, account })
  vi.mocked(runpodApi.catalog).mockResolvedValue(catalog)
  vi.mocked(runpodApi.resources).mockResolvedValue({ pods: [], endpoints: [] })
  vi.mocked(runpodApi.template).mockResolvedValue({ ...catalog.templates[0]!, ports: [{ port: 8888, kind: "http", name: "Jupyter Notebook" }, { port: 22, kind: "tcp", name: "SSH" }] })
  vi.mocked(runpodApi.createPod).mockResolvedValue({} as never)
  vi.mocked(runpodApi.createEndpoint).mockResolvedValue({} as never)
  vi.mocked(runpodApi.hub).mockResolvedValue([{ id: "vllm", title: "vLLM", description: "OpenAI-compatible LLM endpoints", category: "language", owner: "runpod-workers", repo: "worker-vllm", deploys: 52325, stars: 1 }])
  vi.mocked(runpodApi.hubRepo).mockResolvedValue({ id: "vllm", title: "vLLM", description: "", category: "language", owner: "runpod-workers", repo: "worker-vllm", deploys: 1, stars: 1, gpuPools: ["ADA_80_PRO", "AMPERE_80"], gpuCount: 1, runsOn: "GPU", ready: true, presets: [{ name: "tiny", defaults: { MODEL_NAME: "Qwen/Qwen2.5-0.5B-Instruct" } }],
    inputs: [{ key: "MODEL_NAME", name: "Model", type: "huggingface", required: true, advanced: false, default: null, options: [], secret: false }, { key: "TOKENIZER", name: "Tokenizer", type: "string", required: false, advanced: true, default: null, options: [], secret: false }] })
})
afterEach(() => { cleanup(); vi.unstubAllGlobals() })

const next = () => fireEvent.click(screen.getByRole("button", { name: "Continue" }))

describe("Neocloud", () => {
  it("asks only for an API key, installing the tools by itself", async () => {
    vi.mocked(runpodApi.status).mockResolvedValueOnce({ installed: false, connected: false })
    vi.mocked(runpodApi.connect).mockResolvedValueOnce({ installed: true, connected: false, message: "Not signed in" }).mockResolvedValueOnce({ installed: true, connected: true, account })
    render(<NeocloudForm onClose={vi.fn()} />)
    await screen.findByRole("heading", { name: "Rent GPUs in the cloud" })
    expect(runpodApi.connect).toHaveBeenCalledWith()
    fireEvent.change(screen.getByLabelText("API key"), { target: { value: "rpa_secret_key" } })
    fireEvent.click(screen.getByRole("button", { name: "Connect" }))
    await screen.findByRole("heading", { name: "What would you like to create?" })
    expect(runpodApi.connect).toHaveBeenLastCalledWith("rpa_secret_key")
    expect(screen.getByText("$10.00")).toBeTruthy()
  })

  it("says to restart Yougori when the running app predates this screen", async () => {
    vi.mocked(runpodApi.status).mockRejectedValue("Command runpod_status not found")
    render(<NeocloudForm onClose={vi.fn()} />)
    await screen.findByText(/Quit Yougori from the tray and start it again/)
    expect(screen.queryByText(/Getting Neocloud ready/)).toBeNull()
  })

  it("creates a GPU pod from a template at the reviewed price", async () => {
    const close = vi.fn()
    render(<NeocloudForm onClose={close} />)
    fireEvent.click(await screen.findByRole("button", { name: "Create a GPU pod" }))
    fireEvent.click(screen.getByRole("button", { name: "Start from PyTorch 2.8.0" }))
    next()
    expect(screen.queryByRole("button", { name: "Choose A40" })).toBeNull()
    fireEvent.click(screen.getByRole("button", { name: "Choose RTX 4090" }))
    next()
    expect((screen.getByLabelText("Volume size") as HTMLInputElement).value).toBe("50")
    next()
    expect((await screen.findAllByText("$0.74")).length).toBeGreaterThan(0)
    expect(screen.getByText(/covers about 13 hours/)).toBeTruthy()
    const create = screen.getByRole("button", { name: /Create pod/ }) as HTMLButtonElement
    expect(create.disabled).toBe(true)
    fireEvent.click(screen.getByRole("checkbox", { name: /I understand/ }))
    await waitFor(() => expect(create.disabled).toBe(false))
    fireEvent.click(create)
    await waitFor(() => expect(runpodApi.createPod).toHaveBeenCalledOnce())
    expect(runpodApi.createPod).toHaveBeenCalledWith(expect.objectContaining({
      name: "rtx-4090-pytorch-2-8-0", compute: "gpu", gpuId: "NVIDIA GeForce RTX 4090", gpuCount: 1, cloud: "secure", maxHourlyUsd: 0.74,
      templateId: "runpod-torch-v280", containerDiskGb: 30, volumeGb: 50, volumeMountPath: "/workspace", location: "",
    }))
    await waitFor(() => expect(close).toHaveBeenCalled())
  })

  it("deploys a Hub repo as an endpoint with only the settings the user chose", async () => {
    render(<NeocloudForm onClose={vi.fn()} />)
    fireEvent.click(await screen.findByRole("button", { name: "Create a Serverless endpoint" }))
    fireEvent.click(await screen.findByRole("button", { name: "Use vLLM" }))
    await screen.findByText(/· required/)
    expect((screen.getByRole("button", { name: "Continue" }) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(screen.getByRole("button", { name: "Qwen/Qwen2.5-0.5B-Instruct" }))
    next()
    fireEvent.click(await screen.findByRole("checkbox", { name: /I understand/ }))
    fireEvent.click(screen.getByRole("button", { name: "Create endpoint" }))
    await waitFor(() => expect(runpodApi.createEndpoint).toHaveBeenCalledOnce())
    expect(runpodApi.createEndpoint).toHaveBeenCalledWith(expect.objectContaining({ name: "vllm", hubId: "vllm", env: { MODEL_NAME: "Qwen/Qwen2.5-0.5B-Instruct" }, workersMin: 0, workersMax: 1, gpuId: "" }))
  })

  it("creates a CPU pod without a GPU step", async () => {
    render(<NeocloudForm onClose={vi.fn()} />)
    fireEvent.click(await screen.findByRole("button", { name: "Create a CPU pod" }))
    expect(screen.queryByRole("button", { name: "Start from PyTorch 2.8.0" })).toBeNull()
    fireEvent.click(screen.getByRole("button", { name: "Start from Ubuntu 24.04" }))
    next()
    next()
    fireEvent.click(await screen.findByRole("checkbox", { name: /I understand/ }))
    fireEvent.click(screen.getByRole("button", { name: "Create pod" }))
    await waitFor(() => expect(runpodApi.createPod).toHaveBeenCalledWith(expect.objectContaining({ compute: "cpu", templateId: "runpod-ubuntu-2404", volumeGb: 0 })))
    expect(vi.mocked(runpodApi.createPod).mock.calls[0]![0]).not.toHaveProperty("gpuId")
  })

  it("ignores a completed software search after the search was cleared", async () => {
    let finishSearch!: (templates: RunpodCatalog["templates"]) => void
    vi.mocked(runpodApi.searchTemplates).mockImplementation(() => new Promise(resolve => { finishSearch = resolve }))
    render(<NeocloudForm onClose={vi.fn()} />)
    fireEvent.click(await screen.findByRole("button", { name: "Create a GPU pod" }))
    fireEvent.change(screen.getByLabelText("Search software"), { target: { value: "old search" } })
    await waitFor(() => expect(runpodApi.searchTemplates).toHaveBeenCalledWith("old search"))
    fireEvent.change(screen.getByLabelText("Search software"), { target: { value: "" } })
    await act(async () => { finishSearch([{ ...catalog.templates[0]!, id: "old-result", name: "Old result" }]) })
    expect(screen.getByRole("button", { name: "Start from PyTorch 2.8.0" })).toBeTruthy()
    expect(screen.queryByRole("button", { name: "Start from Old result" })).toBeNull()
  })

  it("clears the previous software details while the next template loads", async () => {
    const nextTemplate = { ...catalog.templates[0]!, id: "next-template", name: "Next software" }
    vi.mocked(runpodApi.catalog).mockResolvedValue({ ...catalog, templates: [...catalog.templates, nextTemplate] })
    vi.mocked(runpodApi.template).mockResolvedValueOnce({ ...catalog.templates[0]!, readme: "Previous software instructions" }).mockImplementationOnce(() => new Promise(() => undefined))
    render(<NeocloudForm onClose={vi.fn()} />)
    fireEvent.click(await screen.findByRole("button", { name: "Create a GPU pod" }))
    fireEvent.click(screen.getByRole("button", { name: "Start from PyTorch 2.8.0" }))
    await screen.findByText("Previous software instructions")
    fireEvent.click(screen.getByRole("button", { name: "Start from Next software" }))
    await waitFor(() => expect(runpodApi.template).toHaveBeenLastCalledWith("next-template"))
    expect(screen.queryByText("Previous software instructions")).toBeNull()
  })
})
