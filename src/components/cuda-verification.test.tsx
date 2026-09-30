// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { CudaVerification } from "./cuda-verification"
import { gpuApi, type CudaVerification as Reply } from "@/api/gpu-api"
import type { Environment } from "@/types/platform"

vi.mock("@/api/gpu-api", () => ({ gpuApi: { verifyCuda: vi.fn() } }))
const environment = { id: "cuda-test", provider: "yougoriCuda", kind: "container", status: "running", gpuAccess: true } as Environment
beforeEach(() => { vi.mocked(gpuApi.verifyCuda).mockReset() })
afterEach(cleanup)
it("allows explicitly enabling GPU for old or restored nodes without any graph connector", async () => {
  const enable = vi.fn().mockResolvedValue(undefined)
  render(<CudaVerification environment={{ ...environment, status: "stopped", gpuAccess: false }} onEnableGpu={enable} />)
  fireEvent.click(screen.getByRole("button", { name: "Enable GPU access" }))
  await act(async () => {})
  expect(enable).toHaveBeenCalledOnce()
})
it("shows loading, prevents duplicate tests and displays only a verified result as success", async () => {
  let finish!: (reply: Reply) => void
  vi.mocked(gpuApi.verifyCuda).mockImplementation(() => new Promise(resolve => { finish = resolve }))
  render(<CudaVerification environment={environment} />)
  const button = screen.getByRole("button", { name: "Test CUDA" }) as HTMLButtonElement
  fireEvent.click(button); fireEvent.click(button)
  expect(button.disabled).toBe(true)
  expect(button.getAttribute("aria-busy")).toBe("true")
  expect(gpuApi.verifyCuda).toHaveBeenCalledTimes(1)
  await act(async () => finish({ exitCode: 0, stdout: "CUDA KERNEL PASS: test fixture; 256 GPU results verified", stderr: "" }))
  expect(screen.getByRole("status").textContent).toContain("256 GPU results verified")
  expect(button.disabled).toBe(false)
})
it("does not confuse an adapter listing or a failed command with verified CUDA", async () => {
  vi.mocked(gpuApi.verifyCuda).mockResolvedValue({ exitCode: 0, stdout: "NVIDIA RTX found", stderr: "" })
  render(<CudaVerification environment={environment} />)
  fireEvent.click(screen.getByRole("button", { name: "Test CUDA" }))
  expect((await screen.findByRole("alert")).textContent).toContain("NVIDIA RTX found")
  expect(screen.queryByRole("status")).toBeNull()
})
it("requires a running GPU-enabled CUDA container and does not claim VM support", () => {
  const { rerender } = render(<CudaVerification environment={{ ...environment, gpuAccess: false }} />)
  expect((screen.getByRole("button", { name: "Test CUDA" }) as HTMLButtonElement).disabled).toBe(true)
  rerender(<CudaVerification environment={{ ...environment, status: "stopped" }} />)
  expect((screen.getByRole("button", { name: "Test CUDA" }) as HTMLButtonElement).disabled).toBe(true)
  rerender(<CudaVerification environment={{ ...environment, provider: "qemu", kind: "fullVm" }} />)
  expect(screen.queryByRole("button", { name: "Test CUDA" })).toBeNull()
  expect(screen.getByText(/not CUDA or Windows GPU acceleration/)).toBeTruthy()
  expect(gpuApi.verifyCuda).not.toHaveBeenCalled()
})
