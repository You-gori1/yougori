// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { CudaRuntimePanel } from "./cuda-runtime-panel"
import { gpuApi, type CudaRuntimeStatus } from "@/api/gpu-api"

vi.mock("@/api/gpu-api", () => ({ gpuApi: { cudaStatus: vi.fn(), installCuda: vi.fn() } }))
const status: CudaRuntimeStatus = { supported: true, installed: false, running: false, detail: "Not installed" }
beforeEach(() => { vi.resetAllMocks(); vi.mocked(gpuApi.cudaStatus).mockResolvedValue(status) })
afterEach(cleanup)

it("blocks unsupported computers even with an old installation and allows rechecking after a driver fix", async () => {
  vi.mocked(gpuApi.cudaStatus).mockResolvedValueOnce({ ...status, supported: false, installed: true, detail: "NVIDIA driver not ready", checks: [{ name: "NVIDIA GPU", passed: false, detail: "Install a Windows NVIDIA driver" }] })
  render(<CudaRuntimePanel />)
  await screen.findByText("NVIDIA driver not ready")
  expect((screen.getByRole("button", { name: "Set up CUDA" }) as HTMLButtonElement).disabled).toBe(true)
  expect(screen.queryByText("Installed")).toBeNull()
  expect(screen.getByText("Install a Windows NVIDIA driver")).toBeTruthy()
  vi.mocked(gpuApi.cudaStatus).mockResolvedValueOnce({ ...status, installed: true, detail: "Ready after driver update" })
  fireEvent.click(screen.getByRole("button", { name: "Recheck computer" }))
  expect(await screen.findByText("Installed")).toBeTruthy()
  expect(gpuApi.installCuda).not.toHaveBeenCalled()
})

it("shows setup progress, prevents duplicate downloads, and reports the resulting status", async () => {
  let finish!: (value: CudaRuntimeStatus) => void
  vi.mocked(gpuApi.installCuda).mockImplementation(() => new Promise(resolve => { finish = resolve }))
  const onStatus = vi.fn()
  render(<CudaRuntimePanel onStatus={onStatus} />)
  await screen.findByText("Not installed")
  const button = screen.getByRole("button", { name: "Set up CUDA" }) as HTMLButtonElement
  fireEvent.click(button); fireEvent.click(button)
  expect(button.disabled).toBe(true)
  expect(button.getAttribute("aria-busy")).toBe("true")
  expect(gpuApi.installCuda).toHaveBeenCalledTimes(1)
  await act(async () => finish({ ...status, installed: true, detail: "Ready" }))
  expect(screen.getByText("Installed")).toBeTruthy()
  expect(onStatus).toHaveBeenLastCalledWith(expect.objectContaining({ installed: true }))
})

it("never replaces a running CUDA runtime during an update", async () => {
  vi.mocked(gpuApi.cudaStatus).mockResolvedValue({ ...status, installed: true, running: true, updateAvailable: true, detail: "Close normally before updating" })
  render(<CudaRuntimePanel />)
  const button = await screen.findByRole("button", { name: "Update CUDA" }) as HTMLButtonElement
  expect(button.disabled).toBe(true)
  fireEvent.click(button)
  expect(gpuApi.installCuda).not.toHaveBeenCalled()
})

it("keeps setup errors visible without claiming installation succeeded", async () => {
  vi.mocked(gpuApi.installCuda).mockRejectedValue(new Error("WSL 2 is not ready"))
  render(<CudaRuntimePanel />)
  await screen.findByText("Not installed")
  fireEvent.click(screen.getByRole("button", { name: "Set up CUDA" }))
  expect((await screen.findByRole("alert")).textContent).toContain("WSL 2 is not ready")
  expect(screen.queryByText("Installed")).toBeNull()
})
