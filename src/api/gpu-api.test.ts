// @vitest-environment jsdom
import { beforeEach, expect, it } from "vitest"
import { gpuApi } from "./gpu-api"

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem("yougori.gpu.fixture", JSON.stringify({ adapters: [{ id: "intel", name: "Intel Graphics" }, { id: "nvidia", name: "NVIDIA RTX" }], active: [] }))
})
it("persists explicit selection and can return to Automatic", async () => {
  expect((await gpuApi.select("nvidia")).selectedId).toBe("nvidia")
  expect((await gpuApi.getSettings()).selectedId).toBe("nvidia")
  expect((await gpuApi.select(null)).selectedId).toBeNull()
})
it("never pretends browser preview has installed or tested real CUDA", async () => {
  expect((await gpuApi.cudaStatus()).installed).toBe(false)
  await expect(gpuApi.installCuda()).rejects.toThrow("desktop app")
  await expect(gpuApi.verifyCuda("env-test")).rejects.toThrow("cannot test CUDA")
})
it("rejects unavailable hardware instead of substituting another GPU", async () => {
  await gpuApi.select("intel")
  await expect(gpuApi.select("missing")).rejects.toThrow("unavailable")
  expect((await gpuApi.getSettings()).selectedId).toBe("intel")
})
it("guards paused containers and GPU VMs, without blocking for a stopped VM", async () => {
  for (const environment of [{ kind: "container", status: "paused" }, { kind: "fullVm", status: "running", gpuAccess: true }]) {
    localStorage.setItem("yougori.platform.v1", JSON.stringify({ environments: [environment] }))
    await expect(gpuApi.select("nvidia")).rejects.toThrow("Stop all")
  }
  localStorage.setItem("yougori.platform.v1", JSON.stringify({ environments: [{ kind: "fullVm", status: "stopped", gpuAccess: true }] }))
  await expect(gpuApi.select("nvidia")).resolves.toMatchObject({ selectedId: "nvidia" })
})
