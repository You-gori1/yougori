import { describe, expect, it } from "vitest"
import { customOciImage, defaultOciImage, ociImageGroups, ociImages, ociRegistryLabel, ociStartupCommand } from "@/data/oci-images"

describe("OCI image catalog", () => {
  it("starts services with their image defaults instead of replacing them with sleep", () => {
    for (const image of ociImages) {
      expect(ociStartupCommand(image)).toBe(image.category === "Operating systems" || image.category === "Languages" ? "sleep 2147483647" : "")
    }
  })
  it("offers a broad, unique, grouped catalog", () => {
    const references = ociImages.map((image) => image.value)
    expect(ociImages.length).toBeGreaterThanOrEqual(50)
    expect(new Set(references).size).toBe(references.length)
    expect(ociImageGroups.flatMap((group) => group.items)).toEqual(ociImages)
  })

  it("keeps a runnable default and custom escape hatch", () => {
    expect(defaultOciImage.value).toBe("docker.io/library/alpine:3.24")
    expect(ociImages.at(-1)).toBe(customOciImage)
  })

  it("is visibly diversified across public registries", () => {
    const curated = ociImages.filter((image) => image !== customOciImage)
    const registries = new Set(curated.map((image) => ociRegistryLabel(image.value)))
    const dockerHubShare = curated.filter((image) => image.value.startsWith("docker.io/")).length / curated.length
    expect(registries.size).toBeGreaterThanOrEqual(6)
    expect(dockerHubShare).toBeLessThan(0.75)
  })
})
