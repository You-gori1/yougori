import { platformApi } from "@/api/platform-api"

// File browsing and editing inside an environment, built on the same audited
// guest command path as the terminal. Nothing here touches the PC filesystem.
export interface GuestEntry { name: string; directory: boolean }
export interface GuestDirectory { path: string; entries: GuestEntry[] }

export const maximumEditableBytes = 262144
const chunkBytes = 12288

export function quote(value: string) { return `'${value.replaceAll("'", `'\\''`)}'` }
export function joinPath(directory: string, name: string) {
  return `${directory.replace(/\/+$/, "")}/${name}`.replace(/^$/, "/")
}

function encodeText(value: string) {
  const bytes = new TextEncoder().encode(value)
  let binary = ""
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary)
}

function decodeText(base64: string) {
  const binary = atob(base64.replace(/\s+/g, ""))
  const bytes = Uint8Array.from(binary, character => character.charCodeAt(0))
  if (bytes.includes(0)) throw new Error("This looks like a binary file, so it is not opened in the editor.")
  return new TextDecoder("utf-8", { fatal: false }).decode(bytes)
}

async function shell(environmentId: string, command: string) {
  const result = await platformApi.executeEnvironmentCommand(environmentId, command)
  if (result.exitCode !== 0) throw new Error((result.stderr || result.stdout || "The environment refused this file operation.").trim())
  return result.stdout
}

export const guestFilesApi = {
  async list(environmentId: string, path: string): Promise<GuestDirectory> {
    const target = path.trim() || "."
    // `ls -A -p` marks directories with a trailing slash in both BusyBox and
    // GNU coreutils, so one listing works across container images.
    const output = await shell(environmentId, `cd ${quote(target)} && pwd && ls -A -p`)
    const [resolved, ...names] = output.split("\n")
    const entries = names
      .map(name => name.replace(/\r$/, ""))
      .filter(Boolean)
      .map(name => ({ name: name.replace(/\/$/, ""), directory: name.endsWith("/") }))
      .sort((a, b) => Number(b.directory) - Number(a.directory) || a.name.localeCompare(b.name))
    return { path: (resolved ?? target).trim() || target, entries }
  },

  async read(environmentId: string, path: string) {
    const file = quote(path)
    const size = Number((await shell(environmentId, `wc -c < ${file}`)).trim())
    if (!Number.isFinite(size)) throw new Error("This file could not be measured.")
    if (size > maximumEditableBytes) throw new Error(`This file is larger than ${Math.round(maximumEditableBytes / 1024)} KB, so it is not opened in the editor.`)
    return decodeText(await shell(environmentId, `base64 ${file}`))
  },

  async write(environmentId: string, path: string, contents: string) {
    const encoded = encodeText(contents)
    const file = quote(path)
    // Guest commands are length limited, so a long file is appended in pieces
    // and only replaces the original once every piece has been written.
    const temporary = quote(`${path}.yougori-save`)
    await shell(environmentId, `: > ${temporary}`)
    for (let offset = 0; offset < encoded.length; offset += chunkBytes) {
      await shell(environmentId, `printf %s ${quote(encoded.slice(offset, offset + chunkBytes))} >> ${temporary}`)
    }
    await shell(environmentId, `base64 -d < ${temporary} > ${file} && rm -f ${temporary}`)
  },

  async create(environmentId: string, path: string, directory: boolean) {
    const target = quote(path)
    await shell(environmentId, directory ? `mkdir ${target}` : `set -C && : > ${target}`)
  },

  async remove(environmentId: string, path: string, directory: boolean) {
    await shell(environmentId, directory ? `rmdir ${quote(path)}` : `rm -f -- ${quote(path)}`)
  },
}
