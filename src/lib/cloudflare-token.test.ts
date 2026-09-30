import { describe, expect, it } from "vitest"
import { cloudflareTokenInput } from "./cloudflare-token"

const token = `eyJ${"A".repeat(65)}=`

describe("Cloudflare tunnel token input", () => {
  it("keeps a token pasted on its own", () => {
    expect(cloudflareTokenInput(token)).toBe(token)
  })

  it("extracts the token from Windows and quoted install commands", () => {
    expect(cloudflareTokenInput(`cloudflared.exe service install ${token}`)).toBe(token)
    expect(cloudflareTokenInput(`& "C:\\Program Files\\cloudflared.exe" service install "${token}"`)).toBe(token)
  })

  it("extracts the token from tunnel run commands", () => {
    expect(cloudflareTokenInput(`cloudflared tunnel run --token ${token}`)).toBe(token)
    expect(cloudflareTokenInput(`cloudflared.exe tunnel run --token "${token}"`)).toBe(token)
  })

  it("takes the longest token-shaped value in a copied command", () => {
    expect(cloudflareTokenInput(`cloudflared service install eyJ${"B".repeat(30)} ${token}`)).toBe(token)
  })

  it("accepts other command formats and token prefixes", () => {
    const otherToken = `CF_${"Z".repeat(90)}`
    expect(cloudflareTokenInput(`cloudflared tunnel --token ${otherToken} run`)).toBe(otherToken)
    expect(cloudflareTokenInput(`Copy this credential:\n'${otherToken}'`)).toBe(otherToken)
  })

  it("leaves short or single-value input unchanged for normal validation", () => {
    expect(cloudflareTokenInput("cloudflared.exe service install invalid-token")).toBe("cloudflared.exe service install invalid-token")
    expect(cloudflareTokenInput("invalid-token")).toBe("invalid-token")
  })
})
