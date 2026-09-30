import { expect, it } from "vitest"
import { environmentCategory, environmentLabel } from "./environment-category"
import { allCapabilities } from "@/components/graph-capabilities"
import { defaultGpuImage, gpuImageGroups, gpuImageIssue } from "@/data/gpu-images"

it("classifies existing CUDA containers without changing their isolation or moving data", () => {
  const environment = { kind: "container", provider: "yougoriCuda" } as const
  expect(environmentCategory(environment)).toBe("gpu")
  expect(environmentLabel(environment)).toBe("GPU · NVIDIA CUDA")
  expect(environment.kind).toBe("container")
  expect(environmentCategory({ kind: "fullVm", provider: "qemu" })).toBe("fullVm")
  expect(environmentCategory({ kind: "container", provider: "yougoriOci" })).toBe("container")
  expect(environmentCategory({ kind: "microVm", provider: "qemu" })).toBe("microVm")
  expect(allCapabilities.map(c => c.capability)).toEqual(["internet", "pc"])
})
it("offers glibc bases and rejects known incompatible custom images", () => {
  expect(defaultGpuImage.value).toBe("docker.io/library/ubuntu:24.04")
  expect(gpuImageGroups.flatMap(g => g.items).every(i => !gpuImageIssue(i.value))).toBe(true)
  for (const reference of ["alpine", "docker.io/library/alpine:latest", "python:3.12-alpine", "busybox:latest"]) expect(gpuImageIssue(reference)).toContain("glibc")
  expect(gpuImageIssue("registry.example.com/team/custom:cuda")).toBeNull()
  expect(gpuImageIssue("registry.example.com/alpine-team/custom:cuda")).toBeNull()
})
