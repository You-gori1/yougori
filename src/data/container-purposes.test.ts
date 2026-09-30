import { describe, expect, it } from "vitest"
import { containerPurposes, containerPurposeImage, matchesOciImage } from "@/data/container-purposes"
import { customOciImage, ociImages, ociStartupCommand } from "@/data/oci-images"

describe("container purposes", () => {
  it("maps every purpose to a real curated image with its existing startup behavior", () => {
    expect(new Set(containerPurposes.map(purpose => purpose.id)).size).toBe(containerPurposes.length)
    for (const purpose of containerPurposes) {
      const image = containerPurposeImage(purpose)
      expect(ociImages).toContain(image)
      expect(image).not.toBe(customOciImage)
      expect(matchesOciImage(image, purpose.label)).toBe(true)
      expect(ociStartupCommand(image)).toBe(["database", "static-site"].includes(purpose.id) ? "" : "sleep 2147483647")
    }
  })

  it("searches names, references, registries, categories, and purpose keywords", () => {
    const search = (query: string) => ociImages.filter(image => matchesOciImage(image, query)).map(image => image.value)
    expect(search("  MONGOdb  ")).toContain("docker.io/library/mongo:latest")
    expect(search("node  docker hub")).toEqual(["docker.io/library/node:alpine", "docker.io/library/node:slim"])
    expect(search("next.js")).toContain("docker.io/library/node:slim")
    expect(search("react website")).toContain("docker.io/library/node:alpine")
    expect(search("fastapi")).toContain("docker.io/library/python:slim")
    expect(search("relational database")).toContain("docker.io/library/postgres:latest")
    expect(search("mcr.microsoft.com/dotnet/sdk")).toContain("mcr.microsoft.com/dotnet/sdk:9.0")
    expect(search("operating systems").length).toBeGreaterThan(10)
  })

  it("handles blank queries and no matches without losing the custom option", () => {
    expect(ociImages.filter(image => matchesOciImage(image, " \t "))).toEqual(ociImages)
    expect(ociImages.filter(image => matchesOciImage(image, "nonexistent-test-image-xyz"))).toEqual([])
    expect(matchesOciImage(customOciImage, "custom")).toBe(true)
  })
})
