import { customOciImage, type OciImageGroup, type OciImageOption } from "./oci-images"

// glibc bases work with the built-in probe without bundling a recent CUDA
// toolkit that could unnecessarily exclude older NVIDIA drivers/devices.
export const defaultGpuImage: OciImageOption = { category: "Operating systems", label: "Ubuntu 24.04", value: "docker.io/library/ubuntu:24.04", description: "General GPU workspace. Install your AI tools or framework inside." }
export const gpuImageGroups: OciImageGroup[] = [
  { value: "Operating systems", items: [defaultGpuImage,
    { category: "Operating systems", label: "Ubuntu 22.04", value: "docker.io/library/ubuntu:22.04", description: "Compatible glibc base for GPU applications." },
    { category: "Operating systems", label: "Debian 12", value: "docker.io/library/debian:12-slim", description: "Compact glibc base for GPU services." },
  ] },
  { value: "Languages", items: [
    { category: "Languages", label: "Python 3.12 Slim", value: "docker.io/library/python:3.12-slim", description: "Python workspace. Install a GPU framework compatible with your driver." },
  ] },
  { value: "Developer tools", items: [customOciImage] },
]

export function gpuImageIssue(reference: string): string | null {
  const value = reference.trim().toLowerCase()
  const image = value.split("/").at(-1) ?? value
  if (/(^|[:_-])alpine([:@._-]|$)/.test(image) || /^busybox([:@]|$)/.test(image)) {
    return "Stock Alpine and BusyBox images cannot run the CUDA check. Choose Ubuntu, Debian or a compatible glibc image."
  }
  return null
}
