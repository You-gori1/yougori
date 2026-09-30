import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { platformApi } from "@/api/platform-api"
import { guestFilesApi, joinPath, maximumEditableBytes, quote } from "@/api/guest-files-api"

vi.mock("@/api/platform-api", () => ({ platformApi: { executeEnvironmentCommand: vi.fn() } }))

const execute = vi.mocked(platformApi.executeEnvironmentCommand)
const ok = (stdout: string) => ({ stdout, stderr: "", exitCode: 0 })
const commands = () => execute.mock.calls.map(call => call[1])
beforeEach(() => { execute.mockResolvedValue(ok("")) })
afterEach(() => { vi.resetAllMocks() })

describe("Environment files", () => {
  it("quotes names so a file cannot break out of its command", () => {
    expect(quote("it's here.txt")).toBe(`'it'\\''s here.txt'`)
    expect(quote("a; rm -rf /")).toBe("'a; rm -rf /'")
    expect(joinPath("/workspace/", "app.js")).toBe("/workspace/app.js")
  })

  it("lists directories first and keeps the resolved path", async () => {
    execute.mockResolvedValue(ok("/workspace\nnotes.txt\nsrc/\napp.js\n"))
    const listing = await guestFilesApi.list("env-a", ".")
    expect(listing.path).toBe("/workspace")
    expect(listing.entries).toEqual([
      { name: "src", directory: true },
      { name: "app.js", directory: false },
      { name: "notes.txt", directory: false },
    ])
  })

  it("refuses a file that is too large to edit before reading it", async () => {
    execute.mockResolvedValue(ok(`${maximumEditableBytes + 1}\n`))
    await expect(guestFilesApi.read("env-a", "/workspace/big.bin")).rejects.toThrow(/larger than/)
    expect(commands()).toHaveLength(1)
  })

  it("decodes text and refuses binary content", async () => {
    execute.mockResolvedValueOnce(ok("12\n")).mockResolvedValueOnce(ok(btoa("hello guest\n")))
    expect(await guestFilesApi.read("env-a", "/workspace/notes.txt")).toBe("hello guest\n")
    execute.mockResolvedValueOnce(ok("4\n")).mockResolvedValueOnce(ok(btoa("a\0b\0")))
    await expect(guestFilesApi.read("env-a", "/workspace/app.bin")).rejects.toThrow(/binary/)
  })

  it("writes long files in pieces and only then replaces the original", async () => {
    await guestFilesApi.write("env-a", "/workspace/app.js", "x".repeat(40000))
    const sent = commands()
    expect(sent[0]).toBe(`: > '/workspace/app.js.yougori-save'`)
    expect(sent.filter(command => command.startsWith("printf %s ")).length).toBeGreaterThan(1)
    expect(sent.at(-1)).toBe(`base64 -d < '/workspace/app.js.yougori-save' > '/workspace/app.js' && rm -f '/workspace/app.js.yougori-save'`)
  })

  it("reports the environment's own error instead of claiming success", async () => {
    execute.mockResolvedValue({ stdout: "", stderr: "sh: can't create /etc/passwd: Permission denied", exitCode: 1 })
    await expect(guestFilesApi.create("env-a", "/etc/passwd", false)).rejects.toThrow(/Permission denied/)
  })
})
