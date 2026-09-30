// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest"

afterEach(() => { vi.unstubAllEnvs(); vi.resetModules() })

it("cannot enable fake environments in a production build with the test-adapter flag", async () => {
  vi.stubEnv("PROD", true)
  vi.stubEnv("MODE", "production")
  vi.stubEnv("VITE_YOUGORI_TEST_ADAPTER", "1")
  vi.resetModules()
  const { run } = await import("./platform-api")
  const fixture = vi.fn()
  await expect(run("get_platform_state", undefined, fixture)).rejects.toThrow("native desktop application")
  expect(fixture).not.toHaveBeenCalled()
})

it("continues allowing the explicit development adapter for browser integration tests", async () => {
  vi.stubEnv("PROD", false)
  vi.stubEnv("MODE", "development")
  vi.stubEnv("VITE_YOUGORI_TEST_ADAPTER", "1")
  vi.resetModules()
  const { run } = await import("./platform-api")
  const fixture = vi.fn(() => "test-only")
  await expect(run("get_platform_state", undefined, fixture)).resolves.toBe("test-only")
  expect(fixture).toHaveBeenCalledOnce()
})
