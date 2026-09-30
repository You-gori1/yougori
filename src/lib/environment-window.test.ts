import { describe, expect, it } from "vitest"
import { environmentIdFromSearch } from "@/lib/environment-window"

describe("environment window routing", () => {
  it("reads the environment from the window query", () => {
    expect(environmentIdFromSearch("?environment=env-1234_abcd")).toBe("env-1234_abcd")
    expect(environmentIdFromSearch("?other=value&environment=env-5678")).toBe("env-5678")
  })

  it("rejects missing and empty environment routes", () => {
    expect(environmentIdFromSearch("")).toBeNull()
    expect(environmentIdFromSearch("?environment=%20%20")).toBeNull()
    expect(environmentIdFromSearch("?environment=other")).toBeNull()
    expect(environmentIdFromSearch("?environment=env-..%2Foutside")).toBeNull()
  })
})
