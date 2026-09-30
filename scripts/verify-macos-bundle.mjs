import { spawnSync } from "node:child_process"
import { readFile, readdir } from "node:fs/promises"
import { join, resolve } from "node:path"
import { assertMacCli } from "./release-preflight.mjs"

if (process.platform !== "darwin") throw new Error("Inspect the built .app on macOS")
const app = resolve(process.argv[2] || "src-tauri/target/release/bundle/macos/Yougori.app")
const contents = join(app, "Contents")
for (const file of ["MacOS/yougori", "Resources/cli/yougori", "Resources/cli/yougori-cli", "Resources/cli/opendock-cli"]) {
  assertMacCli(await readFile(join(contents, file)), process.arch)
}
async function inspect(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name)
    if (entry.isSymbolicLink()) throw new Error(`Unexpected symlink in application resources: ${path}`)
    if (entry.isDirectory()) await inspect(path)
    else if (/\.(exe|dll)$/i.test(entry.name)) throw new Error(`Windows binary leaked into macOS package: ${path}`)
  }
}
await inspect(join(contents, "Resources"))
const result = spawnSync("/usr/bin/plutil", ["-lint", join(contents, "Info.plist")], { encoding: "utf8" })
if (result.error || result.status !== 0) throw new Error(result.error?.message || result.stdout || result.stderr)
console.log(`macOS bundle layout and native ${process.arch} binaries verified: ${app}`)
console.log("This is not a Gatekeeper/notarization check; follow docs/macos.md before public distribution.")
