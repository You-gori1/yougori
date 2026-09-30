import { describe, expect, it } from "vitest"
import { accountRequest, emptyCloudflareDraft, savedCloudflareDraft } from "./cloudflare-account"

describe("optional Cloudflare account", () => {
  it("defaults to quick links and remembers account settings when chosen", () => {
    expect(emptyCloudflareDraft()).toEqual({ mode: "quick", hostname: "", localPort: "", token: "", remember: true, routesReviewed: false })
  })
  it("requires an explicit route review and a hostname / fixed bridge port", () => {
    const draft = { ...emptyCloudflareDraft(), hostname: "APP.Example.com", localPort: "45000", token: "test-token", routesReviewed: true }
    expect(accountRequest(draft)).toEqual({ hostPort: 45000, options: { hostname: "app.example.com", token: "test-token", remember: true, routesReviewed: true } })
    const pastedToken = `eyJ${"A".repeat(65)}=`
    expect(accountRequest({ ...draft, token: `cloudflared.exe service install ${pastedToken}` }).options.token).toBe(pastedToken)
    for (const hostname of ["https://app.example.com", "app.example.com:443", "user@app.example.com", "127.0.0.1", "app.local", "app.trycloudflare.com"]) expect(() => accountRequest({ ...draft, hostname })).toThrow()
    for (const localPort of ["", "0", "7443", "65536", "45000abc"]) expect(() => accountRequest({ ...draft, localPort })).toThrow()
    expect(() => accountRequest({ ...draft, routesReviewed: false })).toThrow(/Review/)
  })
  it("asks the backend to reuse a saved token without requesting the secret", () => {
    expect(accountRequest({ ...emptyCloudflareDraft(), hostname: "app.example.com", localPort: "45000", routesReviewed: true }).options.token).toBeUndefined()
  })
  it("restores reviewed account metadata without putting the token in the form", () => {
    const draft = savedCloudflareDraft({ saved: true, hostname: "app.example.com", hostPort: 45000 })!
    expect(draft.mode).toBe("account")
    expect(draft.token).toBe("")
    expect(accountRequest(draft)).toEqual({ hostPort: 45000, options: { hostname: "app.example.com", token: undefined, remember: true, routesReviewed: true } })
    expect(savedCloudflareDraft({ saved: false, hostname: "", hostPort: null })).toBeNull()
    expect(() => savedCloudflareDraft({ saved: true, hostname: "app.example.com", hostPort: null })).toThrow(/port/)
  })
})
