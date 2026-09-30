// Inspect the resources extracted from a newly built installer or installed app.
import { readFile, readdir } from "node:fs/promises"
import { basename, join, resolve } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"
import { checkedFile } from "./compliance-check.mjs"

const noticeName = name => /^(?:LICEN[CS]E|COPYING|COPYRIGHT|NOTICE)(?:[0-9._-]|$)/i.test(name)
  || /[._-](?:LICENSES?|NOTICES?)\.(?:md|txt)$/i.test(name)
  || /[._-](?:COPYING|COPYRIGHT)(?:[0-9._-].*)?$/i.test(name)
const normalized = text => text.replace(/\r\n?/g, "\n")

export async function checkInstalledLicenses(root, installed, platform) {
  if (!["windows", "linux", "macos"].includes(platform)) throw new Error("Choose windows, linux or macos")
  const config = JSON.parse(await readFile(join(root, `src-tauri/tauri.${platform}.conf.json`), "utf8"))
  const files = []
  async function collect(source, destination) {
    for (const entry of await readdir(source, { withFileTypes: true })) {
      if (entry.isSymbolicLink()) throw new Error(`Linked packaging input: ${entry.name}`)
      if (entry.isDirectory()) await collect(join(source, entry.name), join(destination, entry.name))
      else if (noticeName(entry.name)) files.push([join(source, entry.name), join(destination, entry.name)])
    }
  }
  for (const [source, destination] of Object.entries(config.bundle.resources)) {
    const path = resolve(root, "src-tauri", source)
    if (source.endsWith("/")) await collect(path, destination)
    else if (noticeName(basename(source))) files.push([path, destination])
  }
  for (const name of ["LICENSE", "COPYING", "NOTICE", "COMMERCIAL_LICENSE.txt", "THIRD_PARTY_NOTICES.txt", "APPLICATION_LICENSES.txt", "RUNTIME_LICENSES.txt", "WORKSPACE_LICENSES.txt"]) {
    if (!files.some(([, destination]) => destination === name)) throw new Error(`Required installer notice is not configured: ${name}`)
  }
  for (const [expected, destination] of files) {
    const actual = await checkedFile(installed, destination.replaceAll("\\", "/"))
    if (normalized(await readFile(actual, "utf8")) !== normalized(await readFile(expected, "utf8"))) {
      throw new Error(`Installed license/notice differs from reviewed source: ${destination}`)
    }
  }
  return files.length
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [platform, resourceRoot] = process.argv.slice(2)
  if (!resourceRoot) throw new Error("Usage: node scripts/check-installed-licenses.mjs <windows|linux|macos> <extracted-or-installed-resource-directory>")
  const root = fileURLToPath(new URL("..", import.meta.url))
  console.log(`Verified ${await checkInstalledLicenses(root, resolve(resourceRoot), platform)} installed license/notice files against this checkout.`)
}
