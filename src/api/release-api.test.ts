// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from "vitest"
import { releaseApi } from "./release-api"
const { invoke, isTauri } = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn() }))
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri }))
beforeEach(() => { vi.resetAllMocks(); isTauri.mockReturnValue(true); invoke.mockResolvedValue(null) })
it("does not contact the release server in browser previews", async () => {
  isTauri.mockReturnValue(false)
  expect(await releaseApi.check()).toBeNull()
  expect(invoke).not.toHaveBeenCalled()
})
it("routes checks, deferral and the explicit download action through the native backend", async () => {
  await releaseApi.check()
  await releaseApi.check(true)
  await releaseApi.remindLater("1.0.2")
  await releaseApi.openDownloads()
  expect(invoke.mock.calls).toEqual([
    ["check_release_update", { force: false }],
    ["check_release_update", { force: true }],
    ["remind_release_update_later", { version: "1.0.2" }],
    ["open_release_downloads"],
  ])
})
