// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { NeocloudNodeCard } from "./neocloud-node-card"
import { runpodApi } from "@/api/runpod-api"

const deployments = vi.hoisted(() => ({ value: {} as Record<string, unknown> }))
vi.mock("@/api/runpod-api", async importOriginal => ({
  ...await importOriginal<typeof import("@/api/runpod-api")>(),
  runpodApi: { action: vi.fn(), links: vi.fn(), logs: vi.fn(), runEndpoint: vi.fn() },
}))
vi.mock("@/context/platform-context", () => ({ usePlatform: () => ({ state: { environments: [{ id: "env-1", name: "pod-1" }], neocloudDeployments: deployments.value }, refreshPlatform: async () => undefined }) }))
vi.mock("@/api/workspace-api", () => ({ workspaceApi: { openUrl: vi.fn() } }))
vi.mock("@/components/dialogs/cloud-environment-dialog", () => ({ CloudEnvironmentDialog: () => null }))

const base = { provider: "runpod", name: "pod-1", resourceId: "abc123", image: "runpod-torch-v280", offer: "", location: "US-KS-2", address: "1.2.3.4", sshHint: "", requestId: "r", lastError: null }

beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(runpodApi.action).mockResolvedValue({} as never)
  vi.mocked(runpodApi.links).mockResolvedValue({ links: [{ name: "Jupyter Notebook", port: 8888, url: "https://abc123-8888.proxy.runpod.net/lab?token=t" }], console: "https://console.runpod.io/pods?id=abc123" })
})
afterEach(cleanup)

it("shows a running pod's price, Jupyter link and power controls", async () => {
  deployments.value = { "env-1": { ...base, product: "pod", state: "Running", extra: { kind: "pod", compute: "gpu", gpuId: "NVIDIA GeForce RTX 4090", gpuCount: 1, cloud: "secure", hourlyUsd: 0.74, sshReady: true, containerDiskGb: 30, volumeGb: 50, mountPath: "/workspace" } } }
  render(<NeocloudNodeCard environmentId="env-1" />)
  expect(screen.getByRole("status").textContent).toContain("Running · $0.74/hr")
  expect(screen.getByText(/Terminal and files are ready/)).toBeTruthy()
  await screen.findByRole("button", { name: "Open Jupyter" })
  await waitFor(() => expect(runpodApi.action).toHaveBeenCalledWith("env-1", "inspect"))
  fireEvent.click(screen.getByRole("button", { name: "Stop pod" }))
  await waitFor(() => expect(runpodApi.action).toHaveBeenCalledWith("env-1", "stop", undefined))
})

it("gives an endpoint its API addresses and a test request", async () => {
  deployments.value = { "env-1": { ...base, product: "serverless", state: "Ready", extra: { kind: "endpoint", title: "vLLM", category: "language", workersMax: 1, workersMin: 0, idleTimeout: 5,
    urls: { run: "https://api.runpod.ai/v2/abc123/run", runsync: "https://api.runpod.ai/v2/abc123/runsync", health: "", openai: "https://api.runpod.ai/v2/abc123/openai/v1" } } } }
  vi.mocked(runpodApi.runEndpoint).mockResolvedValue({ ok: true, job: { status: "COMPLETED", output: [{ choices: [{ tokens: ["Hello"] }] }] } })
  render(<NeocloudNodeCard environmentId="env-1" />)
  expect(screen.getByText("https://api.runpod.ai/v2/abc123/openai/v1")).toBeTruthy()
  expect(screen.getByRole("button", { name: "Pause" })).toBeTruthy()
  fireEvent.click(screen.getByRole("button", { name: "Send request" }))
  await screen.findByText(/"Hello"/)
  expect(runpodApi.runEndpoint).toHaveBeenCalledWith("env-1", expect.objectContaining({ prompt: "Write a haiku about GPUs" }))
})
