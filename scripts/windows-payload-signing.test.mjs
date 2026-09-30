import { test } from "node:test"
import assert from "node:assert/strict"
import { signWindowsPayload } from "./windows-payload-signing.mjs"

test("development bundling does not request a signing identity", () => {
  signWindowsPayload("C:/repo", "C:/repo/cli.exe", {}, () => { throw new Error("must not run") })
})
test("signed bundling passes paths as arguments and propagates signing failure", () => {
  const env = { YOUGORI_SIGN_CERT_SHA1: "a".repeat(40) }
  assert.throws(() => signWindowsPayload("C:/source with spaces", "C:/output/app.exe", env, (command, args, options) => {
    assert.equal(command, "powershell.exe")
    assert.equal(args.at(-1), "C:/output/app.exe")
    assert.equal(options.env, env)
    assert.equal(options.windowsHide, true)
    return { status: 1 }
  }), /signing failed/)
})
test("a successfully signed payload may proceed to packaging", () => {
  assert.doesNotThrow(() => signWindowsPayload("C:/repo", "C:/repo/cli.exe", { YOUGORI_SIGN_CERT_SHA1: "a".repeat(40) }, () => ({ status: 0 })))
})
