import { workspaceApi } from "@/api/workspace-api"
import { publicAccessPresetScope, readPublicAccessPresets, writePublicAccessPresets, type PublicAccessPreset } from "@/lib/public-access-presets"

export async function rememberPublicAccessPreset(environmentId: string, port: number, hostname: string, hostPort: number): Promise<PublicAccessPreset> {
  const presets = readPublicAccessPresets()
  const sameDomain = presets.find(item => item.hostname.toLowerCase() === hostname.toLowerCase())
  if (sameDomain && (sameDomain.port !== port || sameDomain.hostPort !== hostPort || sameDomain.credentialEnvironmentId !== publicAccessPresetScope)) {
    throw new Error("This domain already has a saved setup with different ports")
  }
  if (presets.some(item => item.id !== sameDomain?.id && item.hostPort === hostPort)) {
    throw new Error("This local tunnel port is already used by another saved setup")
  }
  if (!sameDomain && presets.length >= 200) throw new Error("Remove an unused setup before adding another")

  const preset: PublicAccessPreset = sameDomain ?? { id: crypto.randomUUID(), credentialEnvironmentId: publicAccessPresetScope, port, hostname, hostPort }
  await workspaceApi.copySavedCloudflareToPreset(environmentId, port, preset.id)
  try { await writePublicAccessPresets(sameDomain ? presets : [...presets, preset]) }
  catch (error) {
    if (!sameDomain) await workspaceApi.forgetCloudflarePreset(publicAccessPresetScope, port, preset.id)
    throw error
  }
  return preset
}
