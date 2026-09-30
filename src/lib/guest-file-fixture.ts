import type { CommandResult } from "@/types/platform"

// Browser-only stand-in for a guest filesystem, so the Files tab can be tried
// and tested without a running environment. Never used by the desktop app.
interface GuestDisk { files: Map<string, string>; directories: Set<string> }
const disks = new Map<string, GuestDisk>()
const unquote = (value: string) => value.replace(/^'(.*)'$/s, "$1").replaceAll(`'\\''`, "'")
const parent = (path: string) => path.replace(/\/[^/]*$/, "") || "/"

function guest(environmentId: string): GuestDisk {
  if (!disks.has(environmentId)) {
    disks.set(environmentId, {
      files: new Map([
        ["/workspace/app.js", "export const greeting = 'hello from the environment'\n"],
        ["/workspace/notes.txt", "Edit this file and save it.\n"],
      ]),
      directories: new Set(["/", "/workspace", "/workspace/src"]),
    })
  }
  return disks.get(environmentId)!
}

const ok = (stdout = ""): CommandResult => ({ stdout, stderr: "", exitCode: 0 })
const fail = (stderr: string): CommandResult => ({ stdout: "", stderr, exitCode: 1 })

export function guestFileFixture(environmentId: string, command: string): CommandResult | null {
  const disk = guest(environmentId)
  let match = /^cd (.+) && pwd && ls -A -p$/.exec(command)
  if (match) {
    const requested = unquote(match[1]!)
    const path = requested === "." ? "/workspace" : requested.replace(/\/\.\.$/, "").replace(/\/[^/]*\/\.\.$/, "") || "/"
    const resolved = requested.endsWith("/..") ? parent(requested.slice(0, -3)) : path
    if (!disk.directories.has(resolved)) return fail(`cd: ${resolved}: No such directory`)
    const names = new Set<string>()
    for (const directory of disk.directories) if (parent(directory) === resolved && directory !== resolved) names.add(`${directory.slice(resolved === "/" ? 1 : resolved.length + 1)}/`)
    for (const file of disk.files.keys()) if (parent(file) === resolved) names.add(file.slice(resolved === "/" ? 1 : resolved.length + 1))
    return ok(`${resolved}\n${[...names].join("\n")}\n`)
  }
  match = /^wc -c < (.+)$/.exec(command)
  if (match) {
    const contents = disk.files.get(unquote(match[1]!))
    return contents === undefined ? fail(`${unquote(match[1]!)}: No such file`) : ok(`${new TextEncoder().encode(contents).length}\n`)
  }
  // Checked before the plain read so a decode never looks like a file name.
  match = /^base64 -d < (.+) > (.+) && rm -f (.+)$/.exec(command)
  if (match) {
    const temporary = unquote(match[1]!)
    const bytes = Uint8Array.from(atob(disk.files.get(temporary) ?? ""), character => character.charCodeAt(0))
    disk.files.set(unquote(match[2]!), new TextDecoder().decode(bytes))
    disk.files.delete(temporary)
    return ok()
  }
  match = /^base64 ('(?:[^']|'\\'')*')$/.exec(command)
  if (match) {
    const contents = disk.files.get(unquote(match[1]!))
    if (contents === undefined) return fail("No such file")
    return ok(btoa(String.fromCharCode(...new TextEncoder().encode(contents))))
  }
  match = /^: > (.+)$/.exec(command)
  if (match) { disk.files.set(unquote(match[1]!), ""); return ok() }
  match = /^printf %s (.+) >> (.+)$/.exec(command)
  if (match) { const path = unquote(match[2]!); disk.files.set(path, (disk.files.get(path) ?? "") + unquote(match[1]!)); return ok() }
  match = /^mkdir (.+)$/.exec(command)
  if (match) {
    const path = unquote(match[1]!)
    if (disk.directories.has(path) || disk.files.has(path)) return fail("Already exists")
    disk.directories.add(path); return ok()
  }
  match = /^set -C && : > (.+)$/.exec(command)
  if (match) {
    const path = unquote(match[1]!)
    if (disk.files.has(path) || disk.directories.has(path)) return fail("Already exists")
    disk.files.set(path, ""); return ok()
  }
  match = /^rm -f -- (.+)$/.exec(command)
  if (match) { disk.files.delete(unquote(match[1]!)); return ok() }
  match = /^rmdir (.+)$/.exec(command)
  if (match) {
    const path = unquote(match[1]!)
    if ([...disk.files.keys(), ...disk.directories].some(entry => parent(entry) === path && entry !== path)) return fail("Directory not empty")
    disk.directories.delete(path); return ok()
  }
  return null
}
